use crate::utils::{write_traces, TestEvm};
use alloy_primitives::{bytes, Bytes, U256};
use alloy_sol_types::{sol, SolCall};
use expect_test::expect;
use revm_inspectors::tracing::{types::DecodedCallData, TracingInspector, TracingInspectorConfig};

#[test]
fn test_basic_trace_printing() {
    // solc testdata/Counter.sol --via-ir --optimize --bin
    sol!("testdata/Counter.sol");
    static BYTECODE: Bytes = bytes!("60808060405234601557610415908161001a8239f35b5f80fdfe6080806040526004361015610012575f80fd5b5f905f3560e01c9081630aa7318514610347575080633fb5c1cb14610326578063526f6fc5146102cb57806377fa5d9e1461026e5780638381f58a14610252578063943ee48c146101a85780639db265eb1461014e578063d09de08a146101325763f267ce9e14610081575f80fd5b346101245780600319360112610124576100996103ba565b303b1561012457604051639db265eb60e01b81528190818160048183305af180156101275761010f575b50607b90547f5ae719eb0250b8686767e291df04bec55e7f45a5997e120be020424da1896d766060604051602081526009602082015268343490333937b6901960b91b6040820152a380f35b8161011991610384565b61012457805f6100c3565b80fd5b6040513d84823e3d90fd5b503461012457806003193601126101245761014b6103ba565b80f35b503461012457806003193601126101245780547f9d39c21a43a4dfcd7857f27f3399f31a24694b6cb361496355ab537d16f745ca606060405160208152600960208201526868692066726f6d203360b81b6040820152a280f35b503461024e575f36600319011261024e575f547f9d39c21a43a4dfcd7857f27f3399f31a24694b6cb361496355ab537d16f745ca606060405160208152600960208201526868692066726f6d203160b81b6040820152a2303b1561024e57604051637933e74f60e11b81525f8160048183305af1801561024357610230575b5061014b6103ba565b61023c91505f90610384565b5f80610227565b6040513d5f823e3d90fd5b5f80fd5b3461024e575f36600319011261024e5760205f54604051908152f35b3461024e575f36600319011261024e57607b5f547f5ae719eb0250b8686767e291df04bec55e7f45a5997e120be020424da1896d76606060405160208152600c60208201526b343490333937b6903637b39960a11b6040820152a3005b3461024e575f36600319011261024e575f547f9d39c21a43a4dfcd7857f27f3399f31a24694b6cb361496355ab537d16f745ca606060405160208152600c60208201526b68692066726f6d206c6f673160a01b6040820152a2005b3461024e57602036600319011261024e576004355f55602060405160018152f35b3461024e575f36600319011261024e576080905f54815260406020820152600c60408201526b068692066726f6d206c6f67360a41b6060820152a0005b90601f8019910116810190811067ffffffffffffffff8211176103a657604052565b634e487b7160e01b5f52604160045260245ffd5b5f545f1981146103cb576001015f55565b634e487b7160e01b5f52601160045260245ffdfea26469706673582212203a43412c8d5d5c94f96a961dffefb472490c9ba0f0710d008b49565e95e44daf64736f6c634300081a0033");

    let mut evm = TestEvm::new();

    let mut tracer = TracingInspector::new(TracingInspectorConfig::all().disable_steps());
    let address = evm.deploy(BYTECODE.clone(), &mut tracer).unwrap();

    let mut s = write_traces(&tracer);
    patch_output(&mut s);
    expect![[r#"
        . [209257] → new <unknown>@0xBd770416a3345F91E4B34576cb804a576fa48EB1
            └─ ← [Return] 1045 bytes of code
    "#]]
    .assert_eq(&s);

    let mut call = |data: Vec<u8>| -> String {
        let mut tracer = TracingInspector::new(TracingInspectorConfig::all().disable_steps());
        let r = evm.call(address, data.into(), &mut tracer).unwrap();
        assert!(r.is_success());
        write_traces(&tracer)
    };

    let mut s = call(Counter::numberCall {}.abi_encode());
    patch_output(&mut s);
    expect![[r#"
        . [2348] 0xBd770416a3345F91E4B34576cb804a576fa48EB1::8381f58a()
            └─ ← [Return] 0x0000000000000000000000000000000000000000000000000000000000000000
    "#]]
    .assert_eq(&s);

    let mut s = call(Counter::incrementCall {}.abi_encode());
    patch_output(&mut s);
    expect![[r#"
        . [22452] 0xBd770416a3345F91E4B34576cb804a576fa48EB1::d09de08a()
            └─ ← [Return] 
    "#]]
    .assert_eq(&s);

    let mut s = call(Counter::numberCall {}.abi_encode());
    patch_output(&mut s);
    expect![[r#"
        . [2348] 0xBd770416a3345F91E4B34576cb804a576fa48EB1::8381f58a()
            └─ ← [Return] 0x0000000000000000000000000000000000000000000000000000000000000001
    "#]]
    .assert_eq(&s);

    let mut s = call(Counter::setNumberCall { newNumber: U256::from(69) }.abi_encode());
    patch_output(&mut s);
    expect![[r#"
        . [5189] 0xBd770416a3345F91E4B34576cb804a576fa48EB1::3fb5c1cb(0000000000000000000000000000000000000000000000000000000000000045)
            └─ ← [Return] 0x0000000000000000000000000000000000000000000000000000000000000001
    "#]]
    .assert_eq(&s);

    let mut s = call(Counter::numberCall {}.abi_encode());
    patch_output(&mut s);
    expect![[r#"
        . [2348] 0xBd770416a3345F91E4B34576cb804a576fa48EB1::8381f58a()
            └─ ← [Return] 0x0000000000000000000000000000000000000000000000000000000000000045
    "#]]
    .assert_eq(&s);

    let mut s = call(Counter::nest1Call {}.abi_encode());
    patch_output(&mut s);
    expect![[r#"
        . [13556] 0xBd770416a3345F91E4B34576cb804a576fa48EB1::943ee48c()
            ├─  emit topic 0: 0x9d39c21a43a4dfcd7857f27f3399f31a24694b6cb361496355ab537d16f745ca
            │        topic 1: 0x0000000000000000000000000000000000000000000000000000000000000045
            │           data: 0x0000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000968692066726f6d20310000000000000000000000000000000000000000000000
            ├─ [8575] 0xBd770416a3345F91E4B34576cb804a576fa48EB1::f267ce9e()
            │   ├─ [2337] 0xBd770416a3345F91E4B34576cb804a576fa48EB1::9db265eb()
            │   │   ├─  emit topic 0: 0x9d39c21a43a4dfcd7857f27f3399f31a24694b6cb361496355ab537d16f745ca
            │   │   │        topic 1: 0x0000000000000000000000000000000000000000000000000000000000000046
            │   │   │           data: 0x0000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000968692066726f6d20330000000000000000000000000000000000000000000000
            │   │   └─ ← [Return] 
            │   ├─  emit topic 0: 0x5ae719eb0250b8686767e291df04bec55e7f45a5997e120be020424da1896d76
            │   │        topic 1: 0x0000000000000000000000000000000000000000000000000000000000000046
            │   │        topic 2: 0x000000000000000000000000000000000000000000000000000000000000007b
            │   │           data: 0x0000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000968692066726f6d20320000000000000000000000000000000000000000000000
            │   └─ ← [Return] 
            └─ ← [Return] 
    "#]]
    .assert_eq(&s);

    let mut s = call(Counter::nest2Call {}.abi_encode());
    patch_output(&mut s);
    expect![[r#"
        . [10575] 0xBd770416a3345F91E4B34576cb804a576fa48EB1::f267ce9e()
            ├─ [2337] 0xBd770416a3345F91E4B34576cb804a576fa48EB1::9db265eb()
            │   ├─  emit topic 0: 0x9d39c21a43a4dfcd7857f27f3399f31a24694b6cb361496355ab537d16f745ca
            │   │        topic 1: 0x0000000000000000000000000000000000000000000000000000000000000048
            │   │           data: 0x0000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000968692066726f6d20330000000000000000000000000000000000000000000000
            │   └─ ← [Return] 
            ├─  emit topic 0: 0x5ae719eb0250b8686767e291df04bec55e7f45a5997e120be020424da1896d76
            │        topic 1: 0x0000000000000000000000000000000000000000000000000000000000000048
            │        topic 2: 0x000000000000000000000000000000000000000000000000000000000000007b
            │           data: 0x0000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000968692066726f6d20320000000000000000000000000000000000000000000000
            └─ ← [Return] 
    "#]].assert_eq(&s);

    let mut s = call(Counter::nest3Call {}.abi_encode());
    patch_output(&mut s);
    expect![[r#"
        . [4337] 0xBd770416a3345F91E4B34576cb804a576fa48EB1::9db265eb()
            ├─  emit topic 0: 0x9d39c21a43a4dfcd7857f27f3399f31a24694b6cb361496355ab537d16f745ca
            │        topic 1: 0x0000000000000000000000000000000000000000000000000000000000000048
            │           data: 0x0000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000968692066726f6d20330000000000000000000000000000000000000000000000
            └─ ← [Return] 
    "#]].assert_eq(&s);

    let mut s = call(Counter::log0Call {}.abi_encode());
    patch_output(&mut s);
    expect![[r#"
        . [3711] 0xBd770416a3345F91E4B34576cb804a576fa48EB1::0aa73185()
            ├─           data: 0x00000000000000000000000000000000000000000000000000000000000000480000000000000000000000000000000000000000000000000000000000000040000000000000000000000000000000000000000000000000000000000000000c68692066726f6d206c6f67300000000000000000000000000000000000000000
            └─ ← [Stop] 
    "#]].assert_eq(&s);

    let mut s = call(Counter::log1Call {}.abi_encode());
    patch_output(&mut s);
    expect![[r#"
        . [4242] 0xBd770416a3345F91E4B34576cb804a576fa48EB1::526f6fc5()
            ├─  emit topic 0: 0x9d39c21a43a4dfcd7857f27f3399f31a24694b6cb361496355ab537d16f745ca
            │        topic 1: 0x0000000000000000000000000000000000000000000000000000000000000048
            │           data: 0x0000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000c68692066726f6d206c6f67310000000000000000000000000000000000000000
            └─ ← [Stop] 
    "#]].assert_eq(&s);

    let mut s = call(Counter::log2Call {}.abi_encode());
    patch_output(&mut s);
    expect![[r#"
        . [4642] 0xBd770416a3345F91E4B34576cb804a576fa48EB1::77fa5d9e()
            ├─  emit topic 0: 0x5ae719eb0250b8686767e291df04bec55e7f45a5997e120be020424da1896d76
            │        topic 1: 0x0000000000000000000000000000000000000000000000000000000000000048
            │        topic 2: 0x000000000000000000000000000000000000000000000000000000000000007b
            │           data: 0x0000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000c68692066726f6d206c6f67320000000000000000000000000000000000000000
            └─ ← [Stop] 
    "#]].assert_eq(&s);
}

const LABELS: &[(&str, &str)] = &[("Counter", "0xBd770416a3345F91E4B34576cb804a576fa48EB1")];

// solc testdata/Counter.sol --via-ir --optimize --hashes
const FUNCTION_SELECTORS: &[(&str, &str)] = &[
    ("increment", "0xd09de08a"),
    ("log0", "0x0aa73185"),
    ("log1", "0x526f6fc5"),
    ("log2", "0x77fa5d9e"),
    ("nest1", "0x943ee48c"),
    ("nest2", "0xf267ce9e"),
    ("nest3", "0x9db265eb"),
    ("number", "0x8381f58a"),
    ("setNumber", "0x3fb5c1cb"),
];

// solc testdata/Counter.sol --via-ir --optimize --hashes
const EVENT_SIGNATURES: &[(&str, &str)] = &[
    ("Log1", "0x9d39c21a43a4dfcd7857f27f3399f31a24694b6cb361496355ab537d16f745ca"),
    ("Log2", "0x5ae719eb0250b8686767e291df04bec55e7f45a5997e120be020424da1896d76"),
];

#[test]
fn test_decoded_trace_printing() {
    // solc testdata/Counter.sol --via-ir --optimize --bin
    sol!("testdata/Counter.sol");
    static BYTECODE: Bytes = bytes!("60808060405234601557610415908161001a8239f35b5f80fdfe6080806040526004361015610012575f80fd5b5f905f3560e01c9081630aa7318514610347575080633fb5c1cb14610326578063526f6fc5146102cb57806377fa5d9e1461026e5780638381f58a14610252578063943ee48c146101a85780639db265eb1461014e578063d09de08a146101325763f267ce9e14610081575f80fd5b346101245780600319360112610124576100996103ba565b303b1561012457604051639db265eb60e01b81528190818160048183305af180156101275761010f575b50607b90547f5ae719eb0250b8686767e291df04bec55e7f45a5997e120be020424da1896d766060604051602081526009602082015268343490333937b6901960b91b6040820152a380f35b8161011991610384565b61012457805f6100c3565b80fd5b6040513d84823e3d90fd5b503461012457806003193601126101245761014b6103ba565b80f35b503461012457806003193601126101245780547f9d39c21a43a4dfcd7857f27f3399f31a24694b6cb361496355ab537d16f745ca606060405160208152600960208201526868692066726f6d203360b81b6040820152a280f35b503461024e575f36600319011261024e575f547f9d39c21a43a4dfcd7857f27f3399f31a24694b6cb361496355ab537d16f745ca606060405160208152600960208201526868692066726f6d203160b81b6040820152a2303b1561024e57604051637933e74f60e11b81525f8160048183305af1801561024357610230575b5061014b6103ba565b61023c91505f90610384565b5f80610227565b6040513d5f823e3d90fd5b5f80fd5b3461024e575f36600319011261024e5760205f54604051908152f35b3461024e575f36600319011261024e57607b5f547f5ae719eb0250b8686767e291df04bec55e7f45a5997e120be020424da1896d76606060405160208152600c60208201526b343490333937b6903637b39960a11b6040820152a3005b3461024e575f36600319011261024e575f547f9d39c21a43a4dfcd7857f27f3399f31a24694b6cb361496355ab537d16f745ca606060405160208152600c60208201526b68692066726f6d206c6f673160a01b6040820152a2005b3461024e57602036600319011261024e576004355f55602060405160018152f35b3461024e575f36600319011261024e576080905f54815260406020820152600c60408201526b068692066726f6d206c6f67360a41b6060820152a0005b90601f8019910116810190811067ffffffffffffffff8211176103a657604052565b634e487b7160e01b5f52604160045260245ffd5b5f545f1981146103cb576001015f55565b634e487b7160e01b5f52601160045260245ffdfea26469706673582212203a43412c8d5d5c94f96a961dffefb472490c9ba0f0710d008b49565e95e44daf64736f6c634300081a0033");

    let mut evm = TestEvm::new();

    let mut tracer = TracingInspector::new(TracingInspectorConfig::all().disable_steps());
    let address = evm.deploy(BYTECODE.clone(), &mut tracer).unwrap();

    let mut s = write_traces(&tracer);
    patch_output(&mut s);
    expect![[r#"
        . [209257] → new <unknown>@0xBd770416a3345F91E4B34576cb804a576fa48EB1
            └─ ← [Return] 1045 bytes of code
    "#]]
    .assert_eq(&s);

    let mut index = 0;

    let mut call = |data: Vec<u8>| -> String {
        let mut tracer = TracingInspector::new(TracingInspectorConfig::all().disable_steps());
        let r = evm.call(address, data.into(), &mut tracer).unwrap();
        assert!(r.is_success());

        patch_traces(index, &mut tracer);
        index += 1;

        write_traces(&tracer)
    };

    let mut s = call(Counter::numberCall {}.abi_encode());
    patch_output(&mut s);
    expect![[r#"
        . [2348] Counter::number()
            └─ ← [Return] 0
    "#]]
    .assert_eq(&s);

    let mut s = call(Counter::incrementCall {}.abi_encode());
    patch_output(&mut s);
    expect![[r#"
        . [22452] Counter::increment()
            └─ ← [Return] 
    "#]]
    .assert_eq(&s);

    let mut s = call(Counter::numberCall {}.abi_encode());
    patch_output(&mut s);
    expect![[r#"
        . [2348] Counter::number()
            └─ ← [Return] 1
    "#]]
    .assert_eq(&s);

    let mut s = call(Counter::setNumberCall { newNumber: U256::from(69) }.abi_encode());
    patch_output(&mut s);
    expect![[r#"
        . [5189] Counter::setNumber(69)
            └─ ← [Return] 69
    "#]]
    .assert_eq(&s);

    let mut s = call(Counter::numberCall {}.abi_encode());
    patch_output(&mut s);
    expect![[r#"
        . [2348] Counter::number()
            └─ ← [Return] 69
    "#]]
    .assert_eq(&s);

    let mut s = call(Counter::nest1Call {}.abi_encode());
    patch_output(&mut s);
    expect![[r#"
        . [13556] Counter::nest1()
            ├─ emit Log1(foo: 0x0000000000000000000000000000000000000000000000000000000000000045)
            ├─ [8575] Counter::nest2()
            │   ├─ [2337] Counter::nest3()
            │   │   ├─ emit Log1(foo: 0x0000000000000000000000000000000000000000000000000000000000000046)
            │   │   └─ ← [Return] 
            │   ├─ emit Log2(foo: 0x0000000000000000000000000000000000000000000000000000000000000046, bar: 0x000000000000000000000000000000000000000000000000000000000000007b)
            │   └─ ← [Return] 
            └─ ← [Return] 
    "#]]
    .assert_eq(&s);

    let mut s = call(Counter::nest2Call {}.abi_encode());
    patch_output(&mut s);
    expect![[r#"
        . [10575] Counter::nest2()
            ├─ [2337] Counter::nest3()
            │   ├─ emit Log1(foo: 0x0000000000000000000000000000000000000000000000000000000000000048)
            │   └─ ← [Return] 
            ├─ emit Log2(foo: 0x0000000000000000000000000000000000000000000000000000000000000048, bar: 0x000000000000000000000000000000000000000000000000000000000000007b)
            └─ ← [Return] 
    "#]]
    .assert_eq(&s);

    let mut s = call(Counter::nest3Call {}.abi_encode());
    patch_output(&mut s);
    expect![[r#"
        . [4337] Counter::nest3()
            ├─ emit Log1(foo: 0x0000000000000000000000000000000000000000000000000000000000000048)
            └─ ← [Return] 
    "#]]
    .assert_eq(&s);

    let mut s = call(Counter::log0Call {}.abi_encode());
    patch_output(&mut s);
    expect![[r#"
        . [3711] Counter::log0()
            ├─           data: 0x00000000000000000000000000000000000000000000000000000000000000480000000000000000000000000000000000000000000000000000000000000040000000000000000000000000000000000000000000000000000000000000000c68692066726f6d206c6f67300000000000000000000000000000000000000000
            └─ ← [Stop] 
    "#]].assert_eq(&s);

    let mut s = call(Counter::log1Call {}.abi_encode());
    patch_output(&mut s);
    expect![[r#"
        . [4242] Counter::log1()
            ├─ emit Log1(foo: 0x0000000000000000000000000000000000000000000000000000000000000048)
            └─ ← [Stop] 
    "#]]
    .assert_eq(&s);

    let mut s = call(Counter::log2Call {}.abi_encode());
    patch_output(&mut s);
    expect![[r#"
        . [4642] Counter::log2()
            ├─ emit Log2(foo: 0x0000000000000000000000000000000000000000000000000000000000000048, bar: 0x000000000000000000000000000000000000000000000000000000000000007b)
            └─ ← [Stop] 
    "#]]
    .assert_eq(&s);
}

// Without this, `expect_test` fails on its own updated test output.
fn patch_output(s: &mut str) {
    (unsafe { s[0..1].as_bytes_mut() })[0] = b'.';
}

// Insert decoded fields into the output.
// Note: This is meant to verify that patches are correctly applied to the output.
// The actual decoding logic, including edge case handling, is not implemented here.
fn patch_traces(patch: usize, t: &mut TracingInspector) {
    for node in t.get_traces_mut().nodes_mut() {
        // Inserts decoded `label` into the output, simulating actual decoding.
        LABELS.iter().for_each(|(label, address)| {
            if node.trace.address.to_string() == *address {
                node.trace.decoded.label = Some(label.to_string());
            }
        });

        // Inserts decoded `call_data` into the output, simulating actual decoding.
        FUNCTION_SELECTORS.iter().for_each(|(name, selector)| {
            if node.trace.data.len() == 4 && node.trace.data.to_string().starts_with(*selector) {
                node.trace.decoded.call_data =
                    Some(DecodedCallData { signature: name.to_string(), args: vec![] });
            } else if node.trace.data.len() > 4
                && node.trace.data.to_string().starts_with(*selector)
            {
            }
        });

        // Inserts decoded `name` into the output, simulating actual decoding.
        for log in node.logs.iter_mut() {
            EVENT_SIGNATURES.iter().for_each(|(name, signature)| {
                if !log.raw_log.topics().is_empty()
                    && log.raw_log.topics()[0].to_string() == *signature
                {
                    log.decoded.name = Some(name.to_string());

                    if log.raw_log.topics().len() > 1 {
                        let topics = ["foo", "bar"];
                        log.decoded.params = Some(
                            log.raw_log.topics()[1..]
                                .iter()
                                .zip(topics.iter())
                                .map(|(topic, name)| (name.to_string(), topic.to_string()))
                                .collect(),
                        );
                    }
                }
            });
        }

        // Custom patches for specific traces.
        match patch {
            0 => node.trace.decoded.return_data = Some("0".to_string()),
            2 => node.trace.decoded.return_data = Some("1".to_string()),
            3 => {
                node.trace.decoded.call_data = Some(DecodedCallData {
                    signature: "setNumber".to_string(),
                    args: vec!["69".to_string()],
                });
                node.trace.decoded.return_data = Some("69".to_string())
            }
            4 => node.trace.decoded.return_data = Some("69".to_string()),
            _ => continue,
        }
    }
}
