#!/usr/bin/env bash
set -euo pipefail

# Use the previous ICU dependency set while Socket quarantines newly published releases.
cargo update -p icu_provider --precise 2.3.1
cargo update -p icu_locale_core --precise 2.3.0
cargo update -p tinystr --precise 0.8.4
cargo update -p zerovec --precise 0.11.8
cargo update -p zerovec-derive --precise 0.11.6
cargo update -p yoke --precise 0.8.3
cargo update -p ixdtf --precise 0.6.6
