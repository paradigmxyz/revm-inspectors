// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.8.13;

contract Counter {
    event Log0(uint256 foo, bytes dump) anonymous;
    event Log1(uint256 indexed foo, bytes dump);
    event Log2(uint256 indexed foo, uint256 indexed bar, bytes dump);

    uint256 public number;

    function setNumber(uint256 newNumber) public returns (bool) {
        number = newNumber;
        return true;
    }

    function increment() public {
        number++;
    }

    function log0() public {
        emit Log0(number, "hi from log0");
    }

    function log1() public {
        emit Log1(number, "hi from log1");
    }

    function log2() public {
        emit Log2(number, 123, "hi from log2");
    }

    function nest1() public {
        emit Log1(number, "hi from 1");
        this.nest2();
        increment();
    }

    function nest2() public {
        increment();
        this.nest3();
        emit Log2(number, 123, "hi from 2");
    }

    function nest3() public {
        emit Log1(number, "hi from 3");
    }
}
