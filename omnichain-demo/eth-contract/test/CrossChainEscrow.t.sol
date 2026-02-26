// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Test, console} from "forge-std/Test.sol";
import {CrossChainEscrow} from "../src/CrossChainEscrow.sol";

contract CrossChainEscrowTest is Test {
    CrossChainEscrow public escrow;

    address relayer = address(0x1);
    uint256 relayerKey = 0xA11CE;
    address user1 = address(0x2);
    address user2 = address(0x3);

    uint256 constant TIMEOUT = 3600; // 1 hour
    uint256 constant LOCK_AMOUNT = 1 ether;
    bytes constant PAYLOAD = hex"deadbeef";

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

    function setUp() public {
        // Derive relayer address from key
        relayer = vm.addr(relayerKey);
        escrow = new CrossChainEscrow(relayer, TIMEOUT);

        // Fund test users
        vm.deal(user1, 100 ether);
        vm.deal(user2, 100 ether);
    }

    // ──────────────────────────────────────────────
    // lockFunds tests
    // ──────────────────────────────────────────────

    function test_lockFunds_basic() public {
        vm.prank(user1);
        uint64 n = escrow.lockFunds{value: LOCK_AMOUNT}(PAYLOAD);

        assertEq(n, 1);
        assertEq(escrow.nonce(), 1);

        (address sender, uint256 amount, uint256 deadline, bool executed,, bytes memory payload) =
            escrow.getEscrow(1);

        assertEq(sender, user1);
        assertEq(amount, LOCK_AMOUNT);
        assertEq(deadline, block.timestamp + TIMEOUT);
        assertFalse(executed);
        assertEq(payload, PAYLOAD);
    }

    function test_lockFunds_incrementsNonce() public {
        vm.startPrank(user1);
        uint64 n1 = escrow.lockFunds{value: LOCK_AMOUNT}(PAYLOAD);
        uint64 n2 = escrow.lockFunds{value: LOCK_AMOUNT}(PAYLOAD);
        vm.stopPrank();

        assertEq(n1, 1);
        assertEq(n2, 2);
        assertEq(escrow.nonce(), 2);
    }

    function test_lockFunds_emitsEvent() public {
        vm.prank(user1);
        // We just check that the event is emitted (topic checks)
        vm.expectEmit(false, true, false, false);
        emit CrossChainRequest(bytes32(0), 1, user1, LOCK_AMOUNT, PAYLOAD, 0);
        escrow.lockFunds{value: LOCK_AMOUNT}(PAYLOAD);
    }

    function test_lockFunds_revertsOnZeroValue() public {
        vm.prank(user1);
        vm.expectRevert(CrossChainEscrow.ZeroValue.selector);
        escrow.lockFunds{value: 0}(PAYLOAD);
    }

    function test_lockFunds_revertsOnEmptyPayload() public {
        vm.prank(user1);
        vm.expectRevert(CrossChainEscrow.EmptyPayload.selector);
        escrow.lockFunds{value: LOCK_AMOUNT}("");
    }

    function test_lockFunds_contractBalance() public {
        vm.prank(user1);
        escrow.lockFunds{value: LOCK_AMOUNT}(PAYLOAD);
        assertEq(address(escrow).balance, LOCK_AMOUNT);
    }

    function test_lockFunds_multipleUsers() public {
        vm.prank(user1);
        escrow.lockFunds{value: 1 ether}(PAYLOAD);

        vm.prank(user2);
        escrow.lockFunds{value: 2 ether}(hex"cafe");

        assertEq(address(escrow).balance, 3 ether);

        (address sender1,,,,,) = escrow.getEscrow(1);
        (address sender2,,,,,) = escrow.getEscrow(2);

        assertEq(sender1, user1);
        assertEq(sender2, user2);
    }

    // ──────────────────────────────────────────────
    // settle tests
    // ──────────────────────────────────────────────

    function test_settle_basic() public {
        // Lock funds
        vm.prank(user1);
        escrow.lockFunds{value: LOCK_AMOUNT}(PAYLOAD);

        uint256 balanceBefore = user1.balance;

        // Prepare settlement
        uint64 settleNonce = 1;
        bytes memory result = abi.encodePacked(uint256(2 ether)); // amount * 2

        // Sign the settlement
        bytes32 messageHash = keccak256(abi.encodePacked(settleNonce, result));
        bytes32 ethSignedHash = _toEthSignedMessageHash(messageHash);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(relayerKey, ethSignedHash);
        bytes memory signature = abi.encodePacked(r, s, v);

        // Settle
        vm.prank(relayer);
        escrow.settle(settleNonce, result, signature);

        // Verify
        (,,, bool executed,,) = escrow.getEscrow(1);
        assertTrue(executed);
        assertTrue(escrow.settled(1));
        assertEq(user1.balance, balanceBefore + LOCK_AMOUNT);
    }

    function test_settle_revertsOnNonRelayer() public {
        vm.prank(user1);
        escrow.lockFunds{value: LOCK_AMOUNT}(PAYLOAD);

        bytes memory result = abi.encodePacked(uint256(2 ether));
        bytes32 messageHash = keccak256(abi.encodePacked(uint64(1), result));
        bytes32 ethSignedHash = _toEthSignedMessageHash(messageHash);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(relayerKey, ethSignedHash);
        bytes memory signature = abi.encodePacked(r, s, v);

        vm.prank(user1); // not relayer
        vm.expectRevert(CrossChainEscrow.OnlyRelayer.selector);
        escrow.settle(1, result, signature);
    }

    function test_settle_revertsOnReplay() public {
        vm.prank(user1);
        escrow.lockFunds{value: LOCK_AMOUNT}(PAYLOAD);

        bytes memory result = abi.encodePacked(uint256(2 ether));
        bytes32 messageHash = keccak256(abi.encodePacked(uint64(1), result));
        bytes32 ethSignedHash = _toEthSignedMessageHash(messageHash);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(relayerKey, ethSignedHash);
        bytes memory signature = abi.encodePacked(r, s, v);

        vm.prank(relayer);
        escrow.settle(1, result, signature);

        // Try to settle again — replay
        vm.prank(relayer);
        vm.expectRevert(CrossChainEscrow.AlreadySettled.selector);
        escrow.settle(1, result, signature);
    }

    function test_settle_revertsOnInvalidNonce() public {
        bytes memory result = hex"00";
        bytes memory sig = new bytes(65);

        vm.prank(relayer);
        vm.expectRevert(CrossChainEscrow.InvalidNonce.selector);
        escrow.settle(0, result, sig);

        vm.prank(relayer);
        vm.expectRevert(CrossChainEscrow.InvalidNonce.selector);
        escrow.settle(99, result, sig);
    }

    function test_settle_revertsOnInvalidSignature() public {
        vm.prank(user1);
        escrow.lockFunds{value: LOCK_AMOUNT}(PAYLOAD);

        bytes memory result = abi.encodePacked(uint256(2 ether));

        // Sign with wrong key
        uint256 wrongKey = 0xBAD;
        bytes32 messageHash = keccak256(abi.encodePacked(uint64(1), result));
        bytes32 ethSignedHash = _toEthSignedMessageHash(messageHash);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(wrongKey, ethSignedHash);
        bytes memory signature = abi.encodePacked(r, s, v);

        vm.prank(relayer);
        vm.expectRevert(CrossChainEscrow.InvalidSignature.selector);
        escrow.settle(1, result, signature);
    }

    function test_settle_revertsAfterDeadline() public {
        vm.prank(user1);
        escrow.lockFunds{value: LOCK_AMOUNT}(PAYLOAD);

        // Warp past deadline
        vm.warp(block.timestamp + TIMEOUT + 1);

        bytes memory result = abi.encodePacked(uint256(2 ether));
        bytes32 messageHash = keccak256(abi.encodePacked(uint64(1), result));
        bytes32 ethSignedHash = _toEthSignedMessageHash(messageHash);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(relayerKey, ethSignedHash);
        bytes memory signature = abi.encodePacked(r, s, v);

        vm.prank(relayer);
        vm.expectRevert(CrossChainEscrow.DeadlineExceeded.selector);
        escrow.settle(1, result, signature);
    }

    // ──────────────────────────────────────────────
    // reclaim tests
    // ──────────────────────────────────────────────

    function test_reclaim_afterDeadline() public {
        vm.prank(user1);
        escrow.lockFunds{value: LOCK_AMOUNT}(PAYLOAD);

        uint256 balanceBefore = user1.balance;

        // Warp past deadline
        vm.warp(block.timestamp + TIMEOUT + 1);

        vm.prank(user1);
        escrow.reclaim(1);

        assertEq(user1.balance, balanceBefore + LOCK_AMOUNT);
        (,,, bool executed,,) = escrow.getEscrow(1);
        assertTrue(executed);
    }

    function test_reclaim_revertsBeforeDeadline() public {
        vm.prank(user1);
        escrow.lockFunds{value: LOCK_AMOUNT}(PAYLOAD);

        vm.prank(user1);
        vm.expectRevert(CrossChainEscrow.DeadlineNotReached.selector);
        escrow.reclaim(1);
    }

    function test_reclaim_revertsForNonSender() public {
        vm.prank(user1);
        escrow.lockFunds{value: LOCK_AMOUNT}(PAYLOAD);

        vm.warp(block.timestamp + TIMEOUT + 1);

        vm.prank(user2);
        vm.expectRevert(CrossChainEscrow.OnlySender.selector);
        escrow.reclaim(1);
    }

    function test_reclaim_revertsIfAlreadyExecuted() public {
        vm.prank(user1);
        escrow.lockFunds{value: LOCK_AMOUNT}(PAYLOAD);

        // Settle first
        bytes memory result = abi.encodePacked(uint256(2 ether));
        bytes32 messageHash = keccak256(abi.encodePacked(uint64(1), result));
        bytes32 ethSignedHash = _toEthSignedMessageHash(messageHash);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(relayerKey, ethSignedHash);
        bytes memory signature = abi.encodePacked(r, s, v);

        vm.prank(relayer);
        escrow.settle(1, result, signature);

        // Try to reclaim
        vm.warp(block.timestamp + TIMEOUT + 1);
        vm.prank(user1);
        vm.expectRevert(CrossChainEscrow.AlreadyExecuted.selector);
        escrow.reclaim(1);
    }

    // ──────────────────────────────────────────────
    // Edge cases
    // ──────────────────────────────────────────────

    function test_settleAndReclaimMutualExclusion() public {
        vm.prank(user1);
        escrow.lockFunds{value: LOCK_AMOUNT}(PAYLOAD);

        // Settle
        bytes memory result = abi.encodePacked(uint256(2 ether));
        bytes32 messageHash = keccak256(abi.encodePacked(uint64(1), result));
        bytes32 ethSignedHash = _toEthSignedMessageHash(messageHash);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(relayerKey, ethSignedHash);
        bytes memory signature = abi.encodePacked(r, s, v);

        vm.prank(relayer);
        escrow.settle(1, result, signature);

        // Reclaim should fail
        vm.warp(block.timestamp + TIMEOUT + 1);
        vm.prank(user1);
        vm.expectRevert(CrossChainEscrow.AlreadyExecuted.selector);
        escrow.reclaim(1);
    }

    function testFuzz_lockFunds_anyAmount(uint256 amount) public {
        vm.assume(amount > 0 && amount <= 100 ether);
        vm.deal(user1, amount);

        vm.prank(user1);
        uint64 n = escrow.lockFunds{value: amount}(PAYLOAD);

        (, uint256 escrowed,,,,) = escrow.getEscrow(n);
        assertEq(escrowed, amount);
    }

    // ──────────────────────────────────────────────
    // Helpers
    // ──────────────────────────────────────────────

    function _toEthSignedMessageHash(bytes32 hash) internal pure returns (bytes32) {
        return keccak256(abi.encodePacked("\x19Ethereum Signed Message:\n32", hash));
    }
}
