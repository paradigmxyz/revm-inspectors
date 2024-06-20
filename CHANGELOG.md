# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
