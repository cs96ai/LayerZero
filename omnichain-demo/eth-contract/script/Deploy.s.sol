// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Script, console} from "forge-std/Script.sol";
import {CrossChainEscrow} from "../src/CrossChainEscrow.sol";

contract DeployScript is Script {
    function run() external {
        // Anvil default private key #0
        uint256 deployerKey = vm.envOr(
            "DEPLOYER_PRIVATE_KEY",
            uint256(0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80)
        );
        address relayer = vm.envOr(
            "RELAYER_ADDRESS",
            address(0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266) // Anvil account #0
        );
        uint256 timeout = vm.envOr("ESCROW_TIMEOUT", uint256(3600)); // 1 hour default

        vm.startBroadcast(deployerKey);

        CrossChainEscrow escrow = new CrossChainEscrow(relayer, timeout);

        console.log("CrossChainEscrow deployed at:", address(escrow));
        console.log("Relayer:", relayer);
        console.log("Default timeout:", timeout);

        vm.stopBroadcast();
    }
}
