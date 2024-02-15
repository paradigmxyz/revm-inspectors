// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.8.13;

contract Counter {
    event Log(uint256 indexed number, bytes dump);
    event Log2(uint256 indexed number, bytes dump);

    uint256 public number;

    function setNumber(uint256 newNumber) public returns (bool) {
        number = newNumber;
        return true;
    }

    function increment() public {
        number++;
    }

    function nest1() public {
        emit Log(number, "hi from 1");
        this.nest2();
        increment();
    }

    function nest2() public {
        increment();
        this.nest3();
        emit Log2(number, "hi from 2");
    }

    function nest3() public {
        emit Log(number, "hi from 3");
    }
}
