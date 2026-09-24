# revm-inspectors

Common [`revm`] inspector implementations.

Originally part of [`reth`] as the `reth-revm-inspectors` crate.

## Account extensions

Enable the optional `account-ext` feature when tracing revm accounts with opaque
payloads. It is disabled by default and forwards to `revm/account-ext`; standard
Ethereum trace formats do not include the payload.

## Users

- [`reth`]
- [`foundry`]

[`revm`]: https://github.com/bluealloy/revm/
[`reth`]: https://github.com/paradigmxyz/reth/
[`reth`]: https://github.com/paradigmxyz/reth/
[`foundry`]: https://github.com/foundry-rs/foundry/

#### License

<sup>
Licensed under either of <a href="LICENSE-APACHE">Apache License, Version
2.0</a> or <a href="LICENSE-MIT">MIT license</a> at your option.
</sup>

<br>

<sub>
Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in these crates by you, as defined in the Apache-2.0 license,
shall be dual licensed as above, without any additional terms or conditions.
</sub>
