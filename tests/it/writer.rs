use crate::utils::{write_traces, write_traces_with, write_traces_with_bytecodes, TestEvm};
use alloy_primitives::{address, b256, bytes, hex, Address, B256, U256};
use alloy_sol_types::{sol, SolCall};
use colorchoice::ColorChoice;
use revm_inspectors::tracing::{
    types::{DecodedCallData, DecodedInternalCall, DecodedTraceStep},
    TracingInspector, TracingInspectorConfig,
};
use snapbox::{assert_data_eq, data::DataFormat, str};
use std::path::Path;

const OUT_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/it/writer/");

#[test]
fn test_trace_printing() {
    // solc +0.8.26 testdata/Counter.sol --via-ir --optimize --bin
    sol!("testdata/Counter.sol");
    static CREATION_CODE: &str = "60808060405234601557610415908161001a8239f35b5f80fdfe6080806040526004361015610012575f80fd5b5f905f3560e01c9081630aa7318514610347575080633fb5c1cb14610326578063526f6fc5146102cb57806377fa5d9e1461026e5780638381f58a14610252578063943ee48c146101a85780639db265eb1461014e578063d09de08a146101325763f267ce9e14610081575f80fd5b346101245780600319360112610124576100996103ba565b303b1561012457604051639db265eb60e01b81528190818160048183305af180156101275761010f575b50607b90547f5ae719eb0250b8686767e291df04bec55e7f45a5997e120be020424da1896d766060604051602081526009602082015268343490333937b6901960b91b6040820152a380f35b8161011991610384565b61012457805f6100c3565b80fd5b6040513d84823e3d90fd5b503461012457806003193601126101245761014b6103ba565b80f35b503461012457806003193601126101245780547f9d39c21a43a4dfcd7857f27f3399f31a24694b6cb361496355ab537d16f745ca606060405160208152600960208201526868692066726f6d203360b81b6040820152a280f35b503461024e575f36600319011261024e575f547f9d39c21a43a4dfcd7857f27f3399f31a24694b6cb361496355ab537d16f745ca606060405160208152600960208201526868692066726f6d203160b81b6040820152a2303b1561024e57604051637933e74f60e11b81525f8160048183305af1801561024357610230575b5061014b6103ba565b61023c91505f90610384565b5f80610227565b6040513d5f823e3d90fd5b5f80fd5b3461024e575f36600319011261024e5760205f54604051908152f35b3461024e575f36600319011261024e57607b5f547f5ae719eb0250b8686767e291df04bec55e7f45a5997e120be020424da1896d76606060405160208152600c60208201526b343490333937b6903637b39960a11b6040820152a3005b3461024e575f36600319011261024e575f547f9d39c21a43a4dfcd7857f27f3399f31a24694b6cb361496355ab537d16f745ca606060405160208152600c60208201526b68692066726f6d206c6f673160a01b6040820152a2005b3461024e57602036600319011261024e576004355f55602060405160018152f35b3461024e575f36600319011261024e576080905f54815260406020820152600c60408201526b068692066726f6d206c6f67360a41b6060820152a0005b90601f8019910116810190811067ffffffffffffffff8211176103a657604052565b634e487b7160e01b5f52604160045260245ffd5b5f545f1981146103cb576001015f55565b634e487b7160e01b5f52601160045260245ffdfea2646970667358221220d26cb46e1b195f4ef2e419f8dc457a622eb5066ea0a97b4ab2619d684fe597f764736f6c634300081a0033";
    static DEPLOYED_CODE: &str = "6080806040526004361015610012575f80fd5b5f905f3560e01c9081630aa7318514610347575080633fb5c1cb14610326578063526f6fc5146102cb57806377fa5d9e1461026e5780638381f58a14610252578063943ee48c146101a85780639db265eb1461014e578063d09de08a146101325763f267ce9e14610081575f80fd5b346101245780600319360112610124576100996103ba565b303b1561012457604051639db265eb60e01b81528190818160048183305af180156101275761010f575b50607b90547f5ae719eb0250b8686767e291df04bec55e7f45a5997e120be020424da1896d766060604051602081526009602082015268343490333937b6901960b91b6040820152a380f35b8161011991610384565b61012457805f6100c3565b80fd5b6040513d84823e3d90fd5b503461012457806003193601126101245761014b6103ba565b80f35b503461012457806003193601126101245780547f9d39c21a43a4dfcd7857f27f3399f31a24694b6cb361496355ab537d16f745ca606060405160208152600960208201526868692066726f6d203360b81b6040820152a280f35b503461024e575f36600319011261024e575f547f9d39c21a43a4dfcd7857f27f3399f31a24694b6cb361496355ab537d16f745ca606060405160208152600960208201526868692066726f6d203160b81b6040820152a2303b1561024e57604051637933e74f60e11b81525f8160048183305af1801561024357610230575b5061014b6103ba565b61023c91505f90610384565b5f80610227565b6040513d5f823e3d90fd5b5f80fd5b3461024e575f36600319011261024e5760205f54604051908152f35b3461024e575f36600319011261024e57607b5f547f5ae719eb0250b8686767e291df04bec55e7f45a5997e120be020424da1896d76606060405160208152600c60208201526b343490333937b6903637b39960a11b6040820152a3005b3461024e575f36600319011261024e575f547f9d39c21a43a4dfcd7857f27f3399f31a24694b6cb361496355ab537d16f745ca606060405160208152600c60208201526b68692066726f6d206c6f673160a01b6040820152a2005b3461024e57602036600319011261024e576004355f55602060405160018152f35b3461024e575f36600319011261024e576080905f54815260406020820152600c60408201526b068692066726f6d206c6f67360a41b6060820152a0005b90601f8019910116810190811067ffffffffffffffff8211176103a657604052565b634e487b7160e01b5f52604160045260245ffd5b5f545f1981146103cb576001015f55565b634e487b7160e01b5f52601160045260245ffdfea2646970667358221220d26cb46e1b195f4ef2e419f8dc457a622eb5066ea0a97b4ab2619d684fe597f764736f6c634300081a0033";

    let base_path = Path::new(OUT_DIR).join("test_trace_printing");

    let mut evm = TestEvm::new();

    let mut tracer = TracingInspector::new(TracingInspectorConfig::all().disable_steps());
    let address = evm.deploy(CREATION_CODE.parse().unwrap(), &mut tracer).unwrap();

    let s = write_traces(&tracer);
    assert_data_eq!(
        s,
        str![[r#"
  [209257] → new <unknown>@0xBd770416a3345F91E4B34576cb804a576fa48EB1
    └─ ← [Return] 1045 bytes of code

"#]]
    );

    let s = write_traces_with_bytecodes(&tracer);
    assert_data_eq!(
        &s,
        str![[r#"
  [209257] → new <unknown>@0xBd770416a3345F91E4B34576cb804a576fa48EB1(60808060405234601557610415908161001a8239f35b5f80fdfe6080806040526004361015610012575f80fd5b5f905f3560e01c9081630aa7318514610347575080633fb5c1cb14610326578063526f6fc5146102cb57806377fa5d9e1461026e5780638381f58a14610252578063943ee48c146101a85780639db265eb1461014e578063d09de08a146101325763f267ce9e14610081575f80fd5b346101245780600319360112610124576100996103ba565b303b1561012457604051639db265eb60e01b81528190818160048183305af180156101275761010f575b50607b90547f5ae719eb0250b8686767e291df04bec55e7f45a5997e120be020424da1896d766060604051602081526009602082015268343490333937b6901960b91b6040820152a380f35b8161011991610384565b61012457805f6100c3565b80fd5b6040513d84823e3d90fd5b503461012457806003193601126101245761014b6103ba565b80f35b503461012457806003193601126101245780547f9d39c21a43a4dfcd7857f27f3399f31a24694b6cb361496355ab537d16f745ca606060405160208152600960208201526868692066726f6d203360b81b6040820152a280f35b503461024e575f36600319011261024e575f547f9d39c21a43a4dfcd7857f27f3399f31a24694b6cb361496355ab537d16f745ca606060405160208152600960208201526868692066726f6d203160b81b6040820152a2303b1561024e57604051637933e74f60e11b81525f8160048183305af1801561024357610230575b5061014b6103ba565b61023c91505f90610384565b5f80610227565b6040513d5f823e3d90fd5b5f80fd5b3461024e575f36600319011261024e5760205f54604051908152f35b3461024e575f36600319011261024e57607b5f547f5ae719eb0250b8686767e291df04bec55e7f45a5997e120be020424da1896d76606060405160208152600c60208201526b343490333937b6903637b39960a11b6040820152a3005b3461024e575f36600319011261024e575f547f9d39c21a43a4dfcd7857f27f3399f31a24694b6cb361496355ab537d16f745ca606060405160208152600c60208201526b68692066726f6d206c6f673160a01b6040820152a2005b3461024e57602036600319011261024e576004355f55602060405160018152f35b3461024e575f36600319011261024e576080905f54815260406020820152600c60408201526b068692066726f6d206c6f67360a41b6060820152a0005b90601f8019910116810190811067ffffffffffffffff8211176103a657604052565b634e487b7160e01b5f52604160045260245ffd5b5f545f1981146103cb576001015f55565b634e487b7160e01b5f52601160045260245ffdfea2646970667358221220d26cb46e1b195f4ef2e419f8dc457a622eb5066ea0a97b4ab2619d684fe597f764736f6c634300081a0033)
    └─ ← [Return] 1045 bytes of code (6080806040526004361015610012575f80fd5b5f905f3560e01c9081630aa7318514610347575080633fb5c1cb14610326578063526f6fc5146102cb57806377fa5d9e1461026e5780638381f58a14610252578063943ee48c146101a85780639db265eb1461014e578063d09de08a146101325763f267ce9e14610081575f80fd5b346101245780600319360112610124576100996103ba565b303b1561012457604051639db265eb60e01b81528190818160048183305af180156101275761010f575b50607b90547f5ae719eb0250b8686767e291df04bec55e7f45a5997e120be020424da1896d766060604051602081526009602082015268343490333937b6901960b91b6040820152a380f35b8161011991610384565b61012457805f6100c3565b80fd5b6040513d84823e3d90fd5b503461012457806003193601126101245761014b6103ba565b80f35b503461012457806003193601126101245780547f9d39c21a43a4dfcd7857f27f3399f31a24694b6cb361496355ab537d16f745ca606060405160208152600960208201526868692066726f6d203360b81b6040820152a280f35b503461024e575f36600319011261024e575f547f9d39c21a43a4dfcd7857f27f3399f31a24694b6cb361496355ab537d16f745ca606060405160208152600960208201526868692066726f6d203160b81b6040820152a2303b1561024e57604051637933e74f60e11b81525f8160048183305af1801561024357610230575b5061014b6103ba565b61023c91505f90610384565b5f80610227565b6040513d5f823e3d90fd5b5f80fd5b3461024e575f36600319011261024e5760205f54604051908152f35b3461024e575f36600319011261024e57607b5f547f5ae719eb0250b8686767e291df04bec55e7f45a5997e120be020424da1896d76606060405160208152600c60208201526b343490333937b6903637b39960a11b6040820152a3005b3461024e575f36600319011261024e575f547f9d39c21a43a4dfcd7857f27f3399f31a24694b6cb361496355ab537d16f745ca606060405160208152600c60208201526b68692066726f6d206c6f673160a01b6040820152a2005b3461024e57602036600319011261024e576004355f55602060405160018152f35b3461024e575f36600319011261024e576080905f54815260406020820152600c60408201526b068692066726f6d206c6f67360a41b6060820152a0005b90601f8019910116810190811067ffffffffffffffff8211176103a657604052565b634e487b7160e01b5f52604160045260245ffd5b5f545f1981146103cb576001015f55565b634e487b7160e01b5f52601160045260245ffdfea2646970667358221220d26cb46e1b195f4ef2e419f8dc457a622eb5066ea0a97b4ab2619d684fe597f764736f6c634300081a0033)

"#]]
    );
    assert!(s.contains(&format!("({CREATION_CODE})")), "{s}");
    assert!(s.contains(&format!("({DEPLOYED_CODE})")), "{s}");

    let mut index = 0;

    let mut call = |data: Vec<u8>| {
        let mut tracer = TracingInspector::new(TracingInspectorConfig::all());
        let r = evm.call(address, data.into(), &mut tracer).unwrap();
        assert!(r.is_success(), "evm.call reverted: {r:#?}");

        for color in [false, true] {
            let file_kind = if color { DataFormat::TermSvg } else { DataFormat::Text };
            let extension = if color { "svg" } else { "txt" };
            let color_choice = if color { ColorChoice::Always } else { ColorChoice::Never };
            let assert = |s: &str, extra: &str| {
                let path = base_path.join(format!("{index}{extra}.{extension}"));
                let data = snapbox::Data::read_from(&path, Some(file_kind));
                assert_data_eq!(s, data);
            };

            let s = write_traces_with(&tracer, color_choice, false);
            assert(&s, "");

            patch_traces(index, &mut tracer);
            let s = write_traces_with(&tracer, color_choice, false);
            assert(&s, ".decoded");
        }

        index += 1;
    };

    call(Counter::numberCall {}.abi_encode());

    call(Counter::incrementCall {}.abi_encode());

    call(Counter::numberCall {}.abi_encode());

    call(Counter::setNumberCall { newNumber: U256::from(69) }.abi_encode());

    call(Counter::numberCall {}.abi_encode());

    call(Counter::log2Call {}.abi_encode());

    call(Counter::nest1Call {}.abi_encode());

    call(Counter::nest2Call {}.abi_encode());

    call(Counter::nest3Call {}.abi_encode());

    call(Counter::log0Call {}.abi_encode());

    call(Counter::log1Call {}.abi_encode());

    call(Counter::log2Call {}.abi_encode());
}

#[test]
fn deploy_fail() {
    let mut evm = TestEvm::new();
    let mut tracer = TracingInspector::new(TracingInspectorConfig::all().disable_steps());
    let _ = evm.try_deploy(bytes!("604260005260206000fd"), &mut tracer).unwrap();
    assert_data_eq!(
        write_traces(&tracer),
        str![[r#"
  [18] → new <unknown>@0xBd770416a3345F91E4B34576cb804a576fa48EB1
    └─ ← [Revert] 0x0000000000000000000000000000000000000000000000000000000000000042

"#]]
    );

    tracer.traces_mut().nodes_mut()[0].trace.decoded.return_data = Some("42".to_string());
    assert_data_eq!(
        write_traces(&tracer),
        str![[r#"
  [18] → new <unknown>@0xBd770416a3345F91E4B34576cb804a576fa48EB1
    └─ ← [Revert] 42

"#]]
    );
}

// (name, address)
const LABELS: &[(&str, Address)] =
    &[("Counter", address!("Bd770416a3345F91E4B34576cb804a576fa48EB1"))];

// solc testdata/Counter.sol --via-ir --optimize --hashes
// (name, signature)
const FUNCTION_SELECTORS: &[(&str, [u8; 4])] = &[
    ("increment", hex!("d09de08a")),
    ("log0", hex!("0aa73185")),
    ("log1", hex!("526f6fc5")),
    ("log2", hex!("77fa5d9e")),
    ("nest1", hex!("943ee48c")),
    ("nest2", hex!("f267ce9e")),
    ("nest3", hex!("9db265eb")),
    ("number", hex!("8381f58a")),
    ("setNumber", hex!("3fb5c1cb")),
];

// solc testdata/Counter.sol --via-ir --optimize --hashes
// (name, signature, [params])
const EVENT_SIGNATURES: &[(&str, B256, &[&str])] = &[
    ("Log1", b256!("9d39c21a43a4dfcd7857f27f3399f31a24694b6cb361496355ab537d16f745ca"), &["foo"]),
    (
        "Log2",
        b256!("5ae719eb0250b8686767e291df04bec55e7f45a5997e120be020424da1896d76"),
        &["foo", "bar"],
    ),
];

// Insert decoded fields into the output.
// Note: This is meant to verify that patches are correctly applied to the output.
// The actual decoding logic, including edge case handling, is not implemented here.
fn patch_traces(patch: usize, t: &mut TracingInspector) {
    for node in t.traces_mut().nodes_mut() {
        // Inserts decoded `label` into the output, simulating actual decoding.
        LABELS.iter().for_each(|(label, address)| {
            if node.trace.address == *address {
                node.trace.decoded.label = Some(label.to_string());
            }
        });

        // Inserts decoded `call_data` into the output, simulating actual decoding.
        FUNCTION_SELECTORS.iter().for_each(|(name, selector)| {
            if node.trace.data.starts_with(selector) {
                node.trace.decoded.call_data =
                    Some(DecodedCallData { signature: name.to_string(), args: vec![] });
            }
        });

        // Inserts decoded `name` into the output, simulating actual decoding.
        for log in node.logs.iter_mut() {
            EVENT_SIGNATURES.iter().for_each(|(name, signature, topics)| {
                if log.raw_log.topics().first() == Some(signature) {
                    log.decoded.name = Some(name.to_string());

                    if log.raw_log.topics().len() > 1 {
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
                node.trace.decoded.return_data = Some("1".to_string())
            }
            4 => node.trace.decoded.return_data = Some("69".to_string()),
            5 => {
                node.trace.steps[0].decoded = Some(DecodedTraceStep::Line(
                    "[sload] 0x0000000000000000000000000000000000000000000000000000000000000045"
                        .to_string(),
                ));
            }
            6 if node.trace.depth == 2 => {
                node.trace.steps[30].decoded = Some(DecodedTraceStep::InternalCall(
                    DecodedInternalCall {
                        func_name: "Counter::_nest3Internal".to_string(),
                        args: Some(vec!["arg1".to_string(), "arg2".to_string(), "3".to_string()]),
                        return_data: Some(vec!["ret1".to_string()]),
                    },
                    89,
                ));
                node.trace.steps[87].decoded = Some(DecodedTraceStep::Line("[mstore]".to_string()));
                node.trace.steps[90].decoded =
                    Some(DecodedTraceStep::Line("[before_return]".to_string()));
                println!("{:?}", node.ordering);
            }
            6 if node.trace.depth == 0 => {
                node.trace.steps[10].decoded = Some(DecodedTraceStep::InternalCall(
                    DecodedInternalCall {
                        func_name: "Counter::_nest1".to_string(),
                        args: Some(vec![]),
                        return_data: Some(vec!["ret1".to_string(), "ret2".to_string()]),
                    },
                    150,
                ));
                println!("{:?}", node.ordering);
            }
            _ => continue,
        }
    }
}
