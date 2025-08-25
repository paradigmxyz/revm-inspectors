# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.29.0](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.29.0) - 2025-08-25

### Dependencies

- Bump to revm 29 ([#341](https://github.com/paradigmxyz/revm-inspectors/issues/341))

## [0.28.2](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.28.2) - 2025-08-23

### Miscellaneous Tasks

- Release 0.28.2
- Clippy defense ([#339](https://github.com/paradigmxyz/revm-inspectors/issues/339))
- Add clone to storage inspector ([#340](https://github.com/paradigmxyz/revm-inspectors/issues/340))
- Add default init callframe ([#338](https://github.com/paradigmxyz/revm-inspectors/issues/338))

## [0.28.1](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.28.1) - 2025-08-20

### Features

- [js] Add logic to count current opcode cost instead of cumulative ([#336](https://github.com/paradigmxyz/revm-inspectors/issues/336))

### Miscellaneous Tasks

- Release 0.28.1
- Make fns private ([#337](https://github.com/paradigmxyz/revm-inspectors/issues/337))

## [0.28.0](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.28.0) - 2025-08-12

### Dependencies

- [deps] Bump revm 28.0.0, msrv 1.88 required for revm ([#334](https://github.com/paradigmxyz/revm-inspectors/issues/334))

### Features

- Reused call trace stack ([#325](https://github.com/paradigmxyz/revm-inspectors/issues/325))
- Boxed the decoded field ([#326](https://github.com/paradigmxyz/revm-inspectors/issues/326))
- Updated msrv to 1.86.0 ([#331](https://github.com/paradigmxyz/revm-inspectors/issues/331))

### Miscellaneous Tasks

- Release 0.28.0
- Rm log clone ([#333](https://github.com/paradigmxyz/revm-inspectors/issues/333))
- Decoded cleanups

## [0.27.1](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.27.1) - 2025-07-21

### Miscellaneous Tasks

- Release 0.27.1
- Use hashmap default ([#330](https://github.com/paradigmxyz/revm-inspectors/issues/330))

## [0.27.0](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.27.0) - 2025-07-21

### Bug Fixes

- Allow single digit hex strings ([#329](https://github.com/paradigmxyz/revm-inspectors/issues/329))
- [geth_tracer] Keccak input edge cases ([#328](https://github.com/paradigmxyz/revm-inspectors/issues/328))

### Features

- Add erc7562 config ([#317](https://github.com/paradigmxyz/revm-inspectors/issues/317))
- Geth_erc7562_tracers addition ([#316](https://github.com/paradigmxyz/revm-inspectors/issues/316))
- Use native BigInt with compatibility layer ([#314](https://github.com/paradigmxyz/revm-inspectors/issues/314))

### Miscellaneous Tasks

- Release 0.27.0

### Performance

- Allocate some more initial capacity for CallTraceArena ([#323](https://github.com/paradigmxyz/revm-inspectors/issues/323))
- Optimize push_steps_on_stack to avoid temporary allocation ([#320](https://github.com/paradigmxyz/revm-inspectors/issues/320))
- Pre alloc struct logs ([#319](https://github.com/paradigmxyz/revm-inspectors/issues/319))
- Outline edgecov step fn ([#318](https://github.com/paradigmxyz/revm-inspectors/issues/318))

### Testing

- Add top call revert test ([#312](https://github.com/paradigmxyz/revm-inspectors/issues/312))

## [0.26.5](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.26.5) - 2025-07-03

### Bug Fixes

- Always record revert ([#311](https://github.com/paradigmxyz/revm-inspectors/issues/311))

### Miscellaneous Tasks

- Release 0.26.5

## [0.26.4](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.26.4) - 2025-07-03

### Bug Fixes

- Record manual revert pc ([#310](https://github.com/paradigmxyz/revm-inspectors/issues/310))

### Miscellaneous Tasks

- Release 0.26.4
- Release 0.26.3

## [0.26.2](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.26.2) - 2025-07-03

### Bug Fixes

- Use revert directly ([#309](https://github.com/paradigmxyz/revm-inspectors/issues/309))

### Miscellaneous Tasks

- Release 0.26.2

## [0.26.1](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.26.1) - 2025-07-03

### Bug Fixes

- Record state diffs for vm tracer ([#308](https://github.com/paradigmxyz/revm-inspectors/issues/308))

### Miscellaneous Tasks

- Release 0.26.1

## [0.26.0](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.26.0) - 2025-07-01

### Dependencies

- [deps] Bump to revm 27, alloy 1.2 ([#307](https://github.com/paradigmxyz/revm-inspectors/issues/307))

### Miscellaneous Tasks

- Release 0.26.0
- Add trace_addresses helper ([#306](https://github.com/paradigmxyz/revm-inspectors/issues/306))

## [0.25.0](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.25.0) - 2025-06-20

### Dependencies

- Bump revm v26.0.0 ([#303](https://github.com/paradigmxyz/revm-inspectors/issues/303))

### Miscellaneous Tasks

- Release 0.25.0

## [0.24.0](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.24.0) - 2025-06-13

### Bug Fixes

- Deduct call opcode gas ([#304](https://github.com/paradigmxyz/revm-inspectors/issues/304))

### Miscellaneous Tasks

- Release 0.24.0

## [0.23.1](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.23.1) - 2025-06-07

### Miscellaneous Tasks

- Release 0.23.1
- Remove EOF leftovers ([#301](https://github.com/paradigmxyz/revm-inspectors/issues/301))
- Update deny.toml and upgrade CI workflow ([#302](https://github.com/paradigmxyz/revm-inspectors/issues/302))

## [0.23.0](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.23.0) - 2025-05-23

### Dependencies

- [`deps`] Bump revm to 24.0.0 ([#300](https://github.com/paradigmxyz/revm-inspectors/issues/300))

### Miscellaneous Tasks

- Release 0.23.0
- Remove eof trace handlers ([#299](https://github.com/paradigmxyz/revm-inspectors/issues/299))

## [0.22.3](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.22.3) - 2025-05-19

### Bug Fixes

- Use original bytecodes ([#296](https://github.com/paradigmxyz/revm-inspectors/issues/296))

### Miscellaneous Tasks

- Release 0.22.3
- Make clippy happy ([#297](https://github.com/paradigmxyz/revm-inspectors/issues/297))

## [0.22.2](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.22.2) - 2025-05-16

### Bug Fixes

- Js tracer behavior ([#295](https://github.com/paradigmxyz/revm-inspectors/issues/295))

### Miscellaneous Tasks

- Release 0.22.2

## [0.22.1](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.22.1) - 2025-05-16

### Bug Fixes

- Set error for exit call ([#293](https://github.com/paradigmxyz/revm-inspectors/issues/293))

### Miscellaneous Tasks

- Release 0.22.1

## [0.22.0](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.22.0) - 2025-05-13

### Dependencies

- Bump alloy 1.0.0 ([#290](https://github.com/paradigmxyz/revm-inspectors/issues/290))

### Miscellaneous Tasks

- Release 0.22.0

## [0.21.0](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.21.0) - 2025-05-08

### Bug Fixes

- Correct Ref<[u8]> to &[u8] conversion in FourByteInspector ([#289](https://github.com/paradigmxyz/revm-inspectors/issues/289))

### Dependencies

- Bump revm ([#288](https://github.com/paradigmxyz/revm-inspectors/issues/288))

### Miscellaneous Tasks

- Release 0.21.0

## [0.20.1](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.20.1) - 2025-04-30

### Features

- Added storageinspector ([#285](https://github.com/paradigmxyz/revm-inspectors/issues/285))

### Miscellaneous Tasks

- Release 0.20.1
- [access-list] Add function to access touched slots ([#287](https://github.com/paradigmxyz/revm-inspectors/issues/287))
- Make clippy happy ([#286](https://github.com/paradigmxyz/revm-inspectors/issues/286))

## [0.20.0](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.20.0) - 2025-04-23

### Dependencies

- Bump alloy 0.15 ([#284](https://github.com/paradigmxyz/revm-inspectors/issues/284))

### Miscellaneous Tasks

- Release 0.20.0

## [0.19.1](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.19.1) - 2025-04-16

### Miscellaneous Tasks

- Release 0.19.1

### Other

- Excluded valid 7702 authorities from create_accesslist ([#282](https://github.com/paradigmxyz/revm-inspectors/issues/282))

## [0.19.0](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.19.0) - 2025-04-09

### Miscellaneous Tasks

- Release 0.19.0
- Alloy 0.14 ([#280](https://github.com/paradigmxyz/revm-inspectors/issues/280))
- Derive `Clone` on `TransferInspector` ([#279](https://github.com/paradigmxyz/revm-inspectors/issues/279))

### Other

- Remove DatabaseCommit requirement from JsInspector ContextTr ([#278](https://github.com/paradigmxyz/revm-inspectors/issues/278))

## [0.18.1](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.18.1) - 2025-04-04

### Miscellaneous Tasks

- Release 0.18.1

### Other

- Disable Revm default features ([#277](https://github.com/paradigmxyz/revm-inspectors/issues/277))

### Testing

- Add accesslist tests ([#276](https://github.com/paradigmxyz/revm-inspectors/issues/276))

## [0.18.0](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.18.0) - 2025-03-28

### Bug Fixes

- Use saturating sub for push stack calc ([#274](https://github.com/paradigmxyz/revm-inspectors/issues/274))
- Populate selfdestructs in localized parity ([#273](https://github.com/paradigmxyz/revm-inspectors/issues/273))
- Reversed JUMPI args ([#272](https://github.com/paradigmxyz/revm-inspectors/issues/272))

### Dependencies

- Bump alloy+revm ([#275](https://github.com/paradigmxyz/revm-inspectors/issues/275))
- Bump revm 20.alpha7 ([#270](https://github.com/paradigmxyz/revm-inspectors/issues/270))

### Features

- Add additional constructors for parity trace config ([#269](https://github.com/paradigmxyz/revm-inspectors/issues/269))

### Miscellaneous Tasks

- Release 0.18.0

## [0.16.0](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.16.0) - 2025-03-07

### Dependencies

- Bump alloy 0.12 ([#266](https://github.com/paradigmxyz/revm-inspectors/issues/266))

### Miscellaneous Tasks

- Release 0.16.0

### Other

- Added additional match arm for OutOfFunds ([#265](https://github.com/paradigmxyz/revm-inspectors/issues/265))
- Update utils.rs ([#262](https://github.com/paradigmxyz/revm-inspectors/issues/262))

## [0.15.0](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.15.0) - 2025-01-31

### Bug Fixes

- Fix grammatical errors in documentation ([#254](https://github.com/paradigmxyz/revm-inspectors/issues/254))
- Fix typos ([#253](https://github.com/paradigmxyz/revm-inspectors/issues/253))

### Dependencies

- Bump alloy 0.11 ([#259](https://github.com/paradigmxyz/revm-inspectors/issues/259))

### Features

- Simplify AccessListInspector API ([#256](https://github.com/paradigmxyz/revm-inspectors/issues/256))
- Add edge coverage tracking inspired by AFL/Lucid ([#255](https://github.com/paradigmxyz/revm-inspectors/issues/255))
- Support no_std ([#250](https://github.com/paradigmxyz/revm-inspectors/issues/250))

### Miscellaneous Tasks

- Release 0.15.0
- Fix incorrect function check in mod.rs ([#257](https://github.com/paradigmxyz/revm-inspectors/issues/257))
- [tracer] No whitespace at the end of a line ([#252](https://github.com/paradigmxyz/revm-inspectors/issues/252))

### Other

- Grammar and Clarity Improvements in Code Comments ([#258](https://github.com/paradigmxyz/revm-inspectors/issues/258))

## [0.14.1](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.14.1) - 2024-12-30

### Dependencies

- Matt/bump revm19 ([#251](https://github.com/paradigmxyz/revm-inspectors/issues/251))
- Bump boa 20 ([#247](https://github.com/paradigmxyz/revm-inspectors/issues/247))

### Miscellaneous Tasks

- Release 0.14.1
- Make clippy happy ([#249](https://github.com/paradigmxyz/revm-inspectors/issues/249))

## [0.13.0](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.13.0) - 2024-12-10

### Dependencies

- Bump alloy 0.8 ([#245](https://github.com/paradigmxyz/revm-inspectors/issues/245))

### Miscellaneous Tasks

- Release 0.13.0
- Release 0.13.0

## [0.12.1](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.12.1) - 2024-12-04

### Bug Fixes

- [js] Incorrect caller and contract address extracting ([#244](https://github.com/paradigmxyz/revm-inspectors/issues/244))

### Dependencies

- Bump msrv 1.81 ([#243](https://github.com/paradigmxyz/revm-inspectors/issues/243))

### Miscellaneous Tasks

- Release 0.12.1
- Remove bad todo ([#242](https://github.com/paradigmxyz/revm-inspectors/issues/242))

## [0.12.0](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.12.0) - 2024-11-28

### Dependencies

- Bump alloy ([#241](https://github.com/paradigmxyz/revm-inspectors/issues/241))

### Miscellaneous Tasks

- Release 0.12.0

### Other

- Implement FlatCallTracer ([#240](https://github.com/paradigmxyz/revm-inspectors/issues/240))
- Optimize MuxTracer ([#239](https://github.com/paradigmxyz/revm-inspectors/issues/239))

## [0.11.0](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.11.0) - 2024-11-06

### Dependencies

- Bump revm 18 alloy 0.6 ([#238](https://github.com/paradigmxyz/revm-inspectors/issues/238))

### Features

- [trace/parity] Add  trace creation method ([#237](https://github.com/paradigmxyz/revm-inspectors/issues/237))
- StackSnapshotType (All). ([#235](https://github.com/paradigmxyz/revm-inspectors/issues/235))

### Miscellaneous Tasks

- Release 0.11.0
- Rustmft

## [0.10.0](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.10.0) - 2024-10-23

### Dependencies

- Bump revm ([#236](https://github.com/paradigmxyz/revm-inspectors/issues/236))

### Features

- [prestate] Return code or storage as optional ([#234](https://github.com/paradigmxyz/revm-inspectors/issues/234))

### Miscellaneous Tasks

- Release 0.10.0

## [0.9.0](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.9.0) - 2024-10-18

### Bug Fixes

- [js] The opposite checking logic ([#232](https://github.com/paradigmxyz/revm-inspectors/issues/232))
- [tracing/js] ToHex returns with 0x as prefix ([#226](https://github.com/paradigmxyz/revm-inspectors/issues/226))
- [tracing/js] Error not set in result_fn ([#222](https://github.com/paradigmxyz/revm-inspectors/issues/222))
- [tracing/js] Fault_fn not checked ([#221](https://github.com/paradigmxyz/revm-inspectors/issues/221))
- Record state diffs in `all()` ([#215](https://github.com/paradigmxyz/revm-inspectors/issues/215))

### Dependencies

- Bump alloy 0.5 ([#233](https://github.com/paradigmxyz/revm-inspectors/issues/233))
- Bump revm ([#230](https://github.com/paradigmxyz/revm-inspectors/issues/230))
- Bump revm ([#227](https://github.com/paradigmxyz/revm-inspectors/issues/227))

### Features

- [tests] Make the test code more clear and reuseable ([#225](https://github.com/paradigmxyz/revm-inspectors/issues/225))
- [tracing] Js-tracer add coinbase into context ([#223](https://github.com/paradigmxyz/revm-inspectors/issues/223))
- Tweak write_bytecodes output ([#217](https://github.com/paradigmxyz/revm-inspectors/issues/217))
- Add TraceWriterConfig ([#216](https://github.com/paradigmxyz/revm-inspectors/issues/216))

### Miscellaneous Tasks

- Release 0.9.0
- [tracing/js] Add more unit tests ([#231](https://github.com/paradigmxyz/revm-inspectors/issues/231))
- Simplify JS utils ([#229](https://github.com/paradigmxyz/revm-inspectors/issues/229))
- [tests] Move js tracer into a single module ([#224](https://github.com/paradigmxyz/revm-inspectors/issues/224))
- [meta] Update deny.toml
- [tracing] Return detailed oog message ([#218](https://github.com/paradigmxyz/revm-inspectors/issues/218))

### Other

- Write storage change in trace ([#213](https://github.com/paradigmxyz/revm-inspectors/issues/213))
- Distinguish stack oob error ([#219](https://github.com/paradigmxyz/revm-inspectors/issues/219))

### Testing

- Also test `write_bytecodes` ([#214](https://github.com/paradigmxyz/revm-inspectors/issues/214))
- Writer colors ([#212](https://github.com/paradigmxyz/revm-inspectors/issues/212))

## [0.8.1](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.8.1) - 2024-09-30

### Bug Fixes

- Use alloy maps ([#207](https://github.com/paradigmxyz/revm-inspectors/issues/207))

### Miscellaneous Tasks

- Release 0.8.1

## [0.8.0](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.8.0) - 2024-09-30

### Dependencies

- Bump alloy 0.4 ([#206](https://github.com/paradigmxyz/revm-inspectors/issues/206))

### Miscellaneous Tasks

- Release 0.8.0

## [0.7.7](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.7.7) - 2024-09-24

### Features

- Add convenience conversion ([#204](https://github.com/paradigmxyz/revm-inspectors/issues/204))

### Miscellaneous Tasks

- Release 0.7.7

## [0.7.6](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.7.6) - 2024-09-23

### Features

- Add creation code printing in traces ([#202](https://github.com/paradigmxyz/revm-inspectors/issues/202))

### Miscellaneous Tasks

- Release 0.7.6

## [0.7.5](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.7.5) - 2024-09-23

### Miscellaneous Tasks

- Release 0.7.5
- Add `from_flat_call_config` ([#203](https://github.com/paradigmxyz/revm-inspectors/issues/203))

## [0.7.4](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.7.4) - 2024-09-18

### Miscellaneous Tasks

- Release 0.7.4
- Support flatcall tracer

## [0.7.3](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.7.3) - 2024-09-18

### Bug Fixes

- [tracing] Don't overwrite selfdestruct_address ([#190](https://github.com/paradigmxyz/revm-inspectors/issues/190))

### Miscellaneous Tasks

- Release 0.7.3

## [0.7.2](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.7.2) - 2024-09-15

### Bug Fixes

- [tracing] Align trace output with geth ([#198](https://github.com/paradigmxyz/revm-inspectors/issues/198))

### Miscellaneous Tasks

- Release 0.7.2
- Rm intrusive collections
- Make clippy happy ([#197](https://github.com/paradigmxyz/revm-inspectors/issues/197))

## [0.7.1](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.7.1) - 2024-09-13

### Features

- Add helper for eth_simulateV1 to `TransferInspector` ([#196](https://github.com/paradigmxyz/revm-inspectors/issues/196))

### Miscellaneous Tasks

- Release 0.7.1

## [0.7.0](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.7.0) - 2024-09-11

### Miscellaneous Tasks

- Release 0.7.0
- Add back from owned conversion ([#194](https://github.com/paradigmxyz/revm-inspectors/issues/194))

## [0.6.1](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.6.1) - 2024-09-09

### Bug Fixes

- [tracing/builder] Ensure the selfdestruct trace is at the ending of the same depth ([#192](https://github.com/paradigmxyz/revm-inspectors/issues/192))

### Features

- [tracing/builder] Optimize the trace builder ([#191](https://github.com/paradigmxyz/revm-inspectors/issues/191))

### Miscellaneous Tasks

- Release 0.6.1
- Pin intrusive collections ([#193](https://github.com/paradigmxyz/revm-inspectors/issues/193))
- Flatten alloy-rpc-types ([#189](https://github.com/paradigmxyz/revm-inspectors/issues/189))
- Use msrv 1.79 for clippy

### Other

- Use borrowed Arena in GethTraceBuilder ([#178](https://github.com/paradigmxyz/revm-inspectors/issues/178))

## [0.6.0](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.6.0) - 2024-08-29

### Features

- Trace `position` field + bump deps ([#186](https://github.com/paradigmxyz/revm-inspectors/issues/186))

### Miscellaneous Tasks

- Release 0.6.0

### Other

- Use `code` from `AccountInfo` if it is `Some` ([#185](https://github.com/paradigmxyz/revm-inspectors/issues/185))

## [0.5.7](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.5.7) - 2024-08-22

### Features

- Util method to get selector ([#177](https://github.com/paradigmxyz/revm-inspectors/issues/177))

### Miscellaneous Tasks

- Release 0.5.7
- Chore : update homepage ([#179](https://github.com/paradigmxyz/revm-inspectors/issues/179))

### Other

- Move TransactionContext from js to tracing ([#183](https://github.com/paradigmxyz/revm-inspectors/issues/183))

## [0.5.6](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.5.6) - 2024-08-08

### Dependencies

- Bump revm 13 ([#176](https://github.com/paradigmxyz/revm-inspectors/issues/176))

### Miscellaneous Tasks

- Release 0.5.6
- Update tests

## [0.5.5](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.5.5) - 2024-08-01

### Bug Fixes

- Geth trace inconsistence with selfdestruct ([#173](https://github.com/paradigmxyz/revm-inspectors/issues/173))
- Parity state diff when creating SC with balance ([#172](https://github.com/paradigmxyz/revm-inspectors/issues/172))

### Miscellaneous Tasks

- Release 0.5.5

## [0.5.4](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.5.4) - 2024-07-25

### Bug Fixes

- Gas and gasUsed in trace root only for ParityTrace ([#171](https://github.com/paradigmxyz/revm-inspectors/issues/171))
- Fix Self-destruct Disorder ([#170](https://github.com/paradigmxyz/revm-inspectors/issues/170))

### Miscellaneous Tasks

- Release 0.5.4

## [0.5.3](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.5.3) - 2024-07-19

### Features

- Add immediate bytes recording ([#169](https://github.com/paradigmxyz/revm-inspectors/issues/169))

### Miscellaneous Tasks

- Release 0.5.3
- Release 0.5.2

### Refactor

- Prefer using revm helpers ([#168](https://github.com/paradigmxyz/revm-inspectors/issues/168))

## [0.5.1](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.5.1) - 2024-07-17

### Dependencies

- Bump revm 12.1 ([#167](https://github.com/paradigmxyz/revm-inspectors/issues/167))

### Miscellaneous Tasks

- Release 0.5.1

## [0.5.0](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.5.0) - 2024-07-16

### Bug Fixes

- Include `EOFCreate` in `is_any_create` ([#164](https://github.com/paradigmxyz/revm-inspectors/issues/164))
- Display full revert data when printing CREATE* traces ([#160](https://github.com/paradigmxyz/revm-inspectors/issues/160))

### Dependencies

- Bump revm v12.0.0 ([#166](https://github.com/paradigmxyz/revm-inspectors/issues/166))
- Bump boa 0.19 ([#165](https://github.com/paradigmxyz/revm-inspectors/issues/165))

### Miscellaneous Tasks

- Release 0.5.0

## [0.4.0](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.4.0) - 2024-07-09

### Features

- [EOF] Enable inspector calls ([#156](https://github.com/paradigmxyz/revm-inspectors/issues/156))

### Miscellaneous Tasks

- Release 0.4.0
- Move CODEOWNERS

## [0.3.1](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.3.1) - 2024-07-02

### Features

- Add decoding for individual trace steps ([#157](https://github.com/paradigmxyz/revm-inspectors/issues/157))

### Miscellaneous Tasks

- Release 0.3.1
- Improve opcode filter ([#155](https://github.com/paradigmxyz/revm-inspectors/issues/155))

## [0.3.0](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.3.0) - 2024-06-29

### Bug Fixes

- Track selfdestruct transferred value separately ([#153](https://github.com/paradigmxyz/revm-inspectors/issues/153))

### Features

- [writer] Add support for external decoded data sources ([#151](https://github.com/paradigmxyz/revm-inspectors/issues/151))
- Expose mutable access to tracer config ([#154](https://github.com/paradigmxyz/revm-inspectors/issues/154))

### Miscellaneous Tasks

- Release 0.3.0

### Other

- Optimize memory recording ([#84](https://github.com/paradigmxyz/revm-inspectors/issues/84))

## [0.2.0](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.2.0) - 2024-06-26

### Features

- Small updates for steps tracing ([#152](https://github.com/paradigmxyz/revm-inspectors/issues/152))

### Miscellaneous Tasks

- Release 0.2.0

## [0.1.2](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.1.2) - 2024-06-21

### Features

- Add `Step` to `LogCallOrder` ([#150](https://github.com/paradigmxyz/revm-inspectors/issues/150))

### Miscellaneous Tasks

- Release 0.1.2
- Release 0.1.1

## [0.1.0](https://github.com/paradigmxyz/revm-inspectors/releases/tag/v0.1.0) - 2024-06-20

### Bug Fixes

- Dont treat non existing accounts as created ([#139](https://github.com/paradigmxyz/revm-inspectors/issues/139))
- Use JsUint8Array for buffers ([#134](https://github.com/paradigmxyz/revm-inspectors/issues/134))
- Fix index out of bound when resetting TracingInspector ([#115](https://github.com/paradigmxyz/revm-inspectors/issues/115))
- Selfdestructs once and for all ([#111](https://github.com/paradigmxyz/revm-inspectors/issues/111))
- Always call gas inspector ([#79](https://github.com/paradigmxyz/revm-inspectors/issues/79))
- Use JSON.stringify for JS result ([#71](https://github.com/paradigmxyz/revm-inspectors/issues/71))
- Track dynamic gas used in opcode tracking gas ([#45](https://github.com/paradigmxyz/revm-inspectors/issues/45))
- [tracing] Collect logs only if call succeeded in geth `callTracer` ([#52](https://github.com/paradigmxyz/revm-inspectors/issues/52))
- Record CREATE + SELFDESTRUCT ([#28](https://github.com/paradigmxyz/revm-inspectors/issues/28))
- GetPC typo ([#25](https://github.com/paradigmxyz/revm-inspectors/issues/25))
- Prestate tracer ([#18](https://github.com/paradigmxyz/revm-inspectors/issues/18))
- Use original value in prestate tracer ([#15](https://github.com/paradigmxyz/revm-inspectors/issues/15))
- Apply runtime limits ([#7](https://github.com/paradigmxyz/revm-inspectors/issues/7))
- Use git directly
- Deny

### Dependencies

- Bump revm v10.0 ([#149](https://github.com/paradigmxyz/revm-inspectors/issues/149))
- Bump revm ([#148](https://github.com/paradigmxyz/revm-inspectors/issues/148))
- [deps] Use crates.io alloy ([#147](https://github.com/paradigmxyz/revm-inspectors/issues/147))
- [deps] Bump revm dd98b3b ([#145](https://github.com/paradigmxyz/revm-inspectors/issues/145))
- Bump alloy to 6cb3713 ([#144](https://github.com/paradigmxyz/revm-inspectors/issues/144))
- Bump alloy 00d81d7 ([#141](https://github.com/paradigmxyz/revm-inspectors/issues/141))
- [deps] Bump alloy 14ed25d ([#140](https://github.com/paradigmxyz/revm-inspectors/issues/140))
- Bump alloy ([#138](https://github.com/paradigmxyz/revm-inspectors/issues/138))
- Bump alloy 5796024 ([#137](https://github.com/paradigmxyz/revm-inspectors/issues/137))
- Bump alloy 61140ec ([#135](https://github.com/paradigmxyz/revm-inspectors/issues/135))
- Bump alloy 7320d4c ([#133](https://github.com/paradigmxyz/revm-inspectors/issues/133))
- Bump alloy bd39117
- Bump alloy a28a543 ([#132](https://github.com/paradigmxyz/revm-inspectors/issues/132))
- Bump revm ([#131](https://github.com/paradigmxyz/revm-inspectors/issues/131))
- [deps] Bump alloy 5940871 ([#130](https://github.com/paradigmxyz/revm-inspectors/issues/130))
- [deps] Bump alloy fbd84f8 ([#129](https://github.com/paradigmxyz/revm-inspectors/issues/129))
- Bump alloy f415827 ([#127](https://github.com/paradigmxyz/revm-inspectors/issues/127))
- Bump alloy 07611cf ([#125](https://github.com/paradigmxyz/revm-inspectors/issues/125))
- Bump alloy 792b646 ([#124](https://github.com/paradigmxyz/revm-inspectors/issues/124))
- Bump alloy ([#123](https://github.com/paradigmxyz/revm-inspectors/issues/123))
- Bump alloy ([#122](https://github.com/paradigmxyz/revm-inspectors/issues/122))
- Bump alloy 9d3fa45 ([#121](https://github.com/paradigmxyz/revm-inspectors/issues/121))
- Bump alloy dd7a999 ([#120](https://github.com/paradigmxyz/revm-inspectors/issues/120))
- Bump alloy ([#118](https://github.com/paradigmxyz/revm-inspectors/issues/118))
- Bump revm to 9.0 ([#97](https://github.com/paradigmxyz/revm-inspectors/issues/97))
- Bump alloy ([#117](https://github.com/paradigmxyz/revm-inspectors/issues/117))
- Bump alloy 899fc51 ([#114](https://github.com/paradigmxyz/revm-inspectors/issues/114))
- Bump alloy 77c1240 ([#110](https://github.com/paradigmxyz/revm-inspectors/issues/110))
- Bump alloy 05af0de ([#109](https://github.com/paradigmxyz/revm-inspectors/issues/109))
- Bump alloy ([#108](https://github.com/paradigmxyz/revm-inspectors/issues/108))
- Bump alloy 17c5650 ([#107](https://github.com/paradigmxyz/revm-inspectors/issues/107))
- Bump alloy 0bb7604 ([#106](https://github.com/paradigmxyz/revm-inspectors/issues/106))
- Bump alloy af788af ([#105](https://github.com/paradigmxyz/revm-inspectors/issues/105))
- Bump alloy 4e22b9e ([#102](https://github.com/paradigmxyz/revm-inspectors/issues/102))
- Bump alloy 8808d21 ([#101](https://github.com/paradigmxyz/revm-inspectors/issues/101))
- [deps] Bump to alloy-core to `0.7.1` and alloy to `98da8b8` ([#100](https://github.com/paradigmxyz/revm-inspectors/issues/100))
- Bump alloy 39b8695 ([#99](https://github.com/paradigmxyz/revm-inspectors/issues/99))
- Alloy bump f1b4789 ([#98](https://github.com/paradigmxyz/revm-inspectors/issues/98))
- Bump alloy to 31846e7 ([#96](https://github.com/paradigmxyz/revm-inspectors/issues/96))
- Bump alloy 188c4f8 ([#95](https://github.com/paradigmxyz/revm-inspectors/issues/95))
- Bump alloy rpc deps ([#94](https://github.com/paradigmxyz/revm-inspectors/issues/94))
- Bump alloy rpc types ([#93](https://github.com/paradigmxyz/revm-inspectors/issues/93))
- [deps] Bump alloy 8cb0307 ([#92](https://github.com/paradigmxyz/revm-inspectors/issues/92))
- Bump alloy ([#91](https://github.com/paradigmxyz/revm-inspectors/issues/91))
- Bump alloy 987b393
- Bump alloy ([#90](https://github.com/paradigmxyz/revm-inspectors/issues/90))
- Bump alloy ([#85](https://github.com/paradigmxyz/revm-inspectors/issues/85))
- Bump alloy 17633df ([#83](https://github.com/paradigmxyz/revm-inspectors/issues/83))
- Bump alloy 8c9dd0a ([#82](https://github.com/paradigmxyz/revm-inspectors/issues/82))
- Bump alloy 7d5e42f ([#80](https://github.com/paradigmxyz/revm-inspectors/issues/80))
- Bump alloy ([#78](https://github.com/paradigmxyz/revm-inspectors/issues/78))
- Bump alloy version ([#77](https://github.com/paradigmxyz/revm-inspectors/issues/77))
- [bump] Revm v7.2.0 ([#74](https://github.com/paradigmxyz/revm-inspectors/issues/74))
- Bump MSRV to 1.76 ([#73](https://github.com/paradigmxyz/revm-inspectors/issues/73))
- Bump alloy 410850b ([#72](https://github.com/paradigmxyz/revm-inspectors/issues/72))
- Bump alloy ([#68](https://github.com/paradigmxyz/revm-inspectors/issues/68))
- Revm ([#61](https://github.com/paradigmxyz/revm-inspectors/issues/61))
- Bump alloy ([#43](https://github.com/paradigmxyz/revm-inspectors/issues/43))
- Bump revm ([#42](https://github.com/paradigmxyz/revm-inspectors/issues/42))
- Bump alloy rev ([#31](https://github.com/paradigmxyz/revm-inspectors/issues/31))
- Bump alloy ([#30](https://github.com/paradigmxyz/revm-inspectors/issues/30))
- Bump revm v5.0 ([#29](https://github.com/paradigmxyz/revm-inspectors/issues/29))
- Bump deps ([#26](https://github.com/paradigmxyz/revm-inspectors/issues/26))
- Revert "Revert "dep: lock alloy deps"" ([#23](https://github.com/paradigmxyz/revm-inspectors/issues/23))
- Revert "dep: lock alloy deps" ([#22](https://github.com/paradigmxyz/revm-inspectors/issues/22))
- Lock alloy deps ([#8](https://github.com/paradigmxyz/revm-inspectors/issues/8))
- Bump MSRV to 1.75 to match Alloy ([#19](https://github.com/paradigmxyz/revm-inspectors/issues/19))
- [deps] Bump alloys ([#1](https://github.com/paradigmxyz/revm-inspectors/issues/1))

### Documentation

- Update README.md
- Update CallTrace ([#113](https://github.com/paradigmxyz/revm-inspectors/issues/113))
- Update README ([#87](https://github.com/paradigmxyz/revm-inspectors/issues/87))

### Features

- Add cliff changelog support ([#146](https://github.com/paradigmxyz/revm-inspectors/issues/146))
- Add TracingInspector::into_traces ([#112](https://github.com/paradigmxyz/revm-inspectors/issues/112))
- Derive default for `TracingInspector` ([#104](https://github.com/paradigmxyz/revm-inspectors/issues/104))
- Add transferinspector ([#76](https://github.com/paradigmxyz/revm-inspectors/issues/76))
- Write instruction result when displaying call traces ([#75](https://github.com/paradigmxyz/revm-inspectors/issues/75))
- More geth tracer config functions ([#60](https://github.com/paradigmxyz/revm-inspectors/issues/60))
- [tracing] Implement muxTracer ([#57](https://github.com/paradigmxyz/revm-inspectors/issues/57))
- Add opcode gas iter ([#54](https://github.com/paradigmxyz/revm-inspectors/issues/54))
- Bump alloy rpc types rev ([#53](https://github.com/paradigmxyz/revm-inspectors/issues/53))
- Bump alloy rpc types rev ([#51](https://github.com/paradigmxyz/revm-inspectors/issues/51))
- Bump alloy rpc types rev ([#50](https://github.com/paradigmxyz/revm-inspectors/issues/50))
- Add feature-gated Serde implementations ([#47](https://github.com/paradigmxyz/revm-inspectors/issues/47))
- Upstream trace formatting from Foundry ([#38](https://github.com/paradigmxyz/revm-inspectors/issues/38))
- Add op counter  ([#24](https://github.com/paradigmxyz/revm-inspectors/issues/24))
- Migrate to new inspector API ([#11](https://github.com/paradigmxyz/revm-inspectors/issues/11))
- Use inspector db directly in js ([#9](https://github.com/paradigmxyz/revm-inspectors/issues/9))
- Add TransactionContext type ([#5](https://github.com/paradigmxyz/revm-inspectors/issues/5))
- Fork from `reth-revm-inspectors`

### Miscellaneous Tasks

- Release 0.1.0
- Add Cargo.toml exclude
- Add CODEOWNERS
- Upgrade revm version ([#143](https://github.com/paradigmxyz/revm-inspectors/issues/143))
- Alloy 64feb9b ([#128](https://github.com/paradigmxyz/revm-inspectors/issues/128))
- Always use new_unchecked ([#89](https://github.com/paradigmxyz/revm-inspectors/issues/89))
- Create unknown opcodes as unchecked ([#88](https://github.com/paradigmxyz/revm-inspectors/issues/88))
- [clippy] Allow missing transmute annotations ([#86](https://github.com/paradigmxyz/revm-inspectors/issues/86))
- Migrate to boa18 ([#67](https://github.com/paradigmxyz/revm-inspectors/issues/67))
- Remove inspector stack ([#66](https://github.com/paradigmxyz/revm-inspectors/issues/66))
- Add Inspector::fuse ([#63](https://github.com/paradigmxyz/revm-inspectors/issues/63))
- Remove unused code var ([#56](https://github.com/paradigmxyz/revm-inspectors/issues/56))
- Rename inspector ([#55](https://github.com/paradigmxyz/revm-inspectors/issues/55))
- Remove unused imports ([#48](https://github.com/paradigmxyz/revm-inspectors/issues/48))
- Remove maybeowned inspector ([#44](https://github.com/paradigmxyz/revm-inspectors/issues/44))
- Rename inspector generics ([#33](https://github.com/paradigmxyz/revm-inspectors/issues/33))
- Derive Default for CallTrace ([#32](https://github.com/paradigmxyz/revm-inspectors/issues/32))
- Sort derives ([#35](https://github.com/paradigmxyz/revm-inspectors/issues/35))
- Update call_inspectors macro syntax ([#36](https://github.com/paradigmxyz/revm-inspectors/issues/36))
- [clippy] Make clippy happy ([#27](https://github.com/paradigmxyz/revm-inspectors/issues/27))
- Enforce more lints ([#10](https://github.com/paradigmxyz/revm-inspectors/issues/10))
- Disable default features on revm ([#4](https://github.com/paradigmxyz/revm-inspectors/issues/4))
- Update release.toml

### Other

- Add `AuthCall` variant for `CallKind` ([#103](https://github.com/paradigmxyz/revm-inspectors/issues/103))
- Expose additional fields ([#16](https://github.com/paradigmxyz/revm-inspectors/issues/16))
- Initial commit

### Performance

- Remove GasInspector from tracer, optimize step* ([#142](https://github.com/paradigmxyz/revm-inspectors/issues/142))
- Use Bytes in RecordedMemory ([#126](https://github.com/paradigmxyz/revm-inspectors/issues/126))

### Styling

- Fix `clippy::use_self` ([#34](https://github.com/paradigmxyz/revm-inspectors/issues/34))
- Fmt

### Testing

- Add decode revert test ([#39](https://github.com/paradigmxyz/revm-inspectors/issues/39))

<!-- generated by git-cliff -->
