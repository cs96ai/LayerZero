// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/**
 * @title CrossChainEscrow
 * @notice Escrow contract for cross-chain coordination demo.
 *         Locks funds on Ethereum, emits cross-chain requests,
 *         and settles based on results from a remote chain (Solana).
 *
 * SIMULATION: This contract is part of a local research prototype.
 * - No real tokens cross chain boundaries.
 * - Settlement "proofs" are structurally validated but not cryptographically verified.
 * - The relayer is a single trusted party, not a decentralized validator set.
 */
contract CrossChainEscrow {
    // ──────────────────────────────────────────────
    // Types
    // ──────────────────────────────────────────────

    struct Escrow {
        address sender;
        uint256 amount;
        uint256 deadline;
        bool executed;
        bytes32 traceId;
        bytes payload;
    }

    // ──────────────────────────────────────────────
    // State
    // ──────────────────────────────────────────────

    uint64 public nonce;
    address public relayer;
    uint256 public defaultTimeout;

    mapping(uint64 => Escrow) public escrows;

    // Replay protection: track settled nonces
    mapping(uint64 => bool) public settled;

    // ──────────────────────────────────────────────
    // Events (conform to shared event model)
    // ──────────────────────────────────────────────

    event CrossChainRequest(
        bytes32 indexed traceId,
        uint64 indexed nonce,
        address sender,
        uint256 amount,
        bytes payload,
        uint256 deadline
    );

    event Settled(
        bytes32 indexed traceId,
        uint64 indexed nonce,
        bytes result,
        bool success
    );

    event Reclaimed(
        uint64 indexed nonce,
        address indexed sender,
        uint256 amount
    );

    // ──────────────────────────────────────────────
    // Errors
    // ──────────────────────────────────────────────

    error ZeroValue();
    error EmptyPayload();
    error OnlyRelayer();
    error EscrowNotFound();
    error AlreadyExecuted();
    error AlreadySettled();
    error DeadlineNotReached();
    error DeadlineExceeded();
    error OnlySender();
    error TransferFailed();
    error InvalidSignature();
    error InvalidNonce();

    // ──────────────────────────────────────────────
    // Constructor
    // ──────────────────────────────────────────────

    /**
     * @param _relayer  Address of the trusted relayer
     * @param _timeout  Default timeout in seconds for escrows
     */
    constructor(address _relayer, uint256 _timeout) {
        relayer = _relayer;
        defaultTimeout = _timeout;
    }

    // ──────────────────────────────────────────────
    // External — Lock
    // ──────────────────────────────────────────────

    /**
     * @notice Lock funds and emit a cross-chain request.
     * @param payload Arbitrary bytes forwarded to the remote chain executor.
     * @return currentNonce The nonce assigned to this escrow.
     */
    function lockFunds(bytes calldata payload) external payable returns (uint64 currentNonce) {
        if (msg.value == 0) revert ZeroValue();
        if (payload.length == 0) revert EmptyPayload();

        currentNonce = ++nonce;
        uint256 deadline = block.timestamp + defaultTimeout;

        // Generate a deterministic trace ID from nonce + sender + blockhash
        bytes32 traceId = keccak256(
            abi.encodePacked(currentNonce, msg.sender, blockhash(block.number - 1))
        );

        escrows[currentNonce] = Escrow({
            sender: msg.sender,
            amount: msg.value,
            deadline: deadline,
            executed: false,
            traceId: traceId,
            payload: payload
        });

        emit CrossChainRequest(
            traceId,
            currentNonce,
            msg.sender,
            msg.value,
            payload,
            deadline
        );
    }

    // ──────────────────────────────────────────────
    // External — Settle
    // ──────────────────────────────────────────────

    /**
     * @notice Settle an escrow with results from the remote chain.
     *         Only callable by the trusted relayer.
     * @param _nonce     Nonce of the escrow to settle
     * @param result     Execution result bytes from the remote chain
     * @param signature  Relayer signature over (nonce, result)
     *
     * SIMULATION: The signature check verifies that the relayer signed
     * the settlement data. In a real system, this would verify a threshold
     * signature from a validator set or a light-client proof.
     */
    function settle(
        uint64 _nonce,
        bytes calldata result,
        bytes calldata signature
    ) external {
        if (msg.sender != relayer) revert OnlyRelayer();
        if (_nonce == 0 || _nonce > nonce) revert InvalidNonce();
        if (settled[_nonce]) revert AlreadySettled();

        Escrow storage escrow = escrows[_nonce];
        if (escrow.sender == address(0)) revert EscrowNotFound();
        if (escrow.executed) revert AlreadyExecuted();
        if (block.timestamp > escrow.deadline) revert DeadlineExceeded();

        // SIMULATION: Verify relayer signature over (nonce, result).
        // In production, this would be a light-client proof or multi-sig verification.
        bytes32 messageHash = keccak256(abi.encodePacked(_nonce, result));
        bytes32 ethSignedHash = _toEthSignedMessageHash(messageHash);
        address signer = _recoverSigner(ethSignedHash, signature);
        if (signer != relayer) revert InvalidSignature();

        // Mark as executed and settled
        escrow.executed = true;
        settled[_nonce] = true;

        // Release funds back to sender (in a real bridge, funds might go elsewhere)
        (bool success,) = escrow.sender.call{value: escrow.amount}("");
        if (!success) revert TransferFailed();

        emit Settled(escrow.traceId, _nonce, result, true);
    }

    // ──────────────────────────────────────────────
    // External — Reclaim (timeout)
    // ──────────────────────────────────────────────

    /**
     * @notice Reclaim escrowed funds after the deadline has passed
     *         without settlement.
     * @param _nonce Nonce of the escrow to reclaim
     */
    function reclaim(uint64 _nonce) external {
        Escrow storage escrow = escrows[_nonce];
        if (escrow.sender == address(0)) revert EscrowNotFound();
        if (msg.sender != escrow.sender) revert OnlySender();
        if (escrow.executed) revert AlreadyExecuted();
        if (block.timestamp < escrow.deadline) revert DeadlineNotReached();

        escrow.executed = true;

        (bool success,) = escrow.sender.call{value: escrow.amount}("");
        if (!success) revert TransferFailed();

        emit Reclaimed(_nonce, escrow.sender, escrow.amount);
    }

    // ──────────────────────────────────────────────
    // View
    // ──────────────────────────────────────────────

    function getEscrow(uint64 _nonce)
        external
        view
        returns (
            address sender,
            uint256 amount,
            uint256 deadline,
            bool executed,
            bytes32 traceId,
            bytes memory payload
        )
    {
        Escrow storage e = escrows[_nonce];
        return (e.sender, e.amount, e.deadline, e.executed, e.traceId, e.payload);
    }

    // ──────────────────────────────────────────────
    // Internal — Signature helpers
    // ──────────────────────────────────────────────

    function _toEthSignedMessageHash(bytes32 hash) internal pure returns (bytes32) {
        return keccak256(abi.encodePacked("\x19Ethereum Signed Message:\n32", hash));
    }

    function _recoverSigner(bytes32 ethSignedHash, bytes memory sig)
        internal
        pure
        returns (address)
    {
        if (sig.length != 65) return address(0);

        bytes32 r;
        bytes32 s;
        uint8 v;

        assembly {
            r := mload(add(sig, 32))
            s := mload(add(sig, 64))
            v := byte(0, mload(add(sig, 96)))
        }

        if (v < 27) v += 27;
        if (v != 27 && v != 28) return address(0);

        return ecrecover(ethSignedHash, v, r, s);
    }
}
