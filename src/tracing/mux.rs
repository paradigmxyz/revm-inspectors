use crate::tracing::{
    js::{JsInspector, JsInspectorError},
    FourByteInspector, TracingInspector, TracingInspectorConfig,
};
use alloc::vec::Vec;
use alloy_primitives::{map::HashMap, Address, Log, U256};
use alloy_rpc_types_eth::TransactionInfo;
use alloy_rpc_types_trace::geth::{
    mux::{MuxConfig, MuxFrame},
    CallConfig, FlatCallConfig, FourByteFrame, GethDebugBuiltInTracerType, GethDebugTracerType,
    GethTrace, NoopFrame, PreStateConfig,
};
use revm::{
    context::{Block, Transaction},
    context_interface::{
        result::{HaltReasonTr, ResultAndState},
        ContextTr,
    },
    inspector::JournalExt,
    interpreter::{
        CallInputs, CallOutcome, CreateInputs, CreateOutcome, EOFCreateInputs, Interpreter,
    },
    DatabaseRef, Inspector,
};
use thiserror::Error;

/// Mux tracing inspector that runs and collects results of multiple inspectors at once.
#[derive(Debug)]
#[cfg(feature = "js-tracer")]
pub struct MuxInspector {
    /// An instance of FourByteInspector that can be reused
    four_byte: Option<FourByteInspector>,
    /// An instance of JsInspector that can be reused
    js_tracer: Option<JsInspector>,
    /// An instance of TracingInspector that can be reused
    tracing: Option<TracingInspector>,
    /// Configurations for different Geth trace types
    configs: Vec<(GethDebugBuiltInTracerType, TraceConfig)>,
}

/// Holds all Geth supported trace configurations
#[derive(Clone, Debug)]
enum TraceConfig {
    Call(CallConfig),
    PreState(PreStateConfig),
    FlatCall(FlatCallConfig),
    Noop,
}

impl MuxInspector {
    /// Try creating a new instance of [MuxInspector] from the given [MuxConfig].
    pub fn try_from_config(config: MuxConfig) -> Result<MuxInspector, Error> {
        let mut four_byte = None;
        let mut js_tracer = None;
        let mut inspector_config = TracingInspectorConfig::none();
        let mut configs = Vec::new();

        // Process each tracer configuration
        for (tracer_type, tracer_config) in config.0 {
            match tracer_type {
                GethDebugTracerType::BuiltInTracer(built_in_tracer_type) => {
                    match built_in_tracer_type {
                        GethDebugBuiltInTracerType::FourByteTracer => {
                            if tracer_config.is_some() {
                                return Err(Error::UnexpectedConfig(tracer_type));
                            }
                            four_byte = Some(FourByteInspector::default());
                        }
                        GethDebugBuiltInTracerType::CallTracer => {
                            let call_config = tracer_config
                                .ok_or(Error::MissingConfig(built_in_tracer_type))?
                                .into_call_config()?;

                            inspector_config
                                .merge(TracingInspectorConfig::from_geth_call_config(&call_config));
                            configs.push((built_in_tracer_type, TraceConfig::Call(call_config)));
                        }
                        GethDebugBuiltInTracerType::PreStateTracer => {
                            let prestate_config = tracer_config
                                .ok_or(Error::MissingConfig(built_in_tracer_type))?
                                .into_pre_state_config()?;

                            inspector_config.merge(
                                TracingInspectorConfig::from_geth_prestate_config(&prestate_config),
                            );
                            configs.push((
                                built_in_tracer_type,
                                TraceConfig::PreState(prestate_config),
                            ));
                        }
                        GethDebugBuiltInTracerType::NoopTracer => {
                            if tracer_config.is_some() {
                                return Err(Error::UnexpectedConfig(tracer_type));
                            }
                            configs.push((built_in_tracer_type, TraceConfig::Noop));
                        }
                        GethDebugBuiltInTracerType::FlatCallTracer => {
                            let flatcall_config = tracer_config
                                .ok_or(Error::MissingConfig(built_in_tracer_type))?
                                .into_flat_call_config()?;

                            inspector_config.merge(TracingInspectorConfig::from_flat_call_config(
                                &flatcall_config,
                            ));
                            configs.push((
                                built_in_tracer_type,
                                TraceConfig::FlatCall(flatcall_config),
                            ));
                        }
                        GethDebugBuiltInTracerType::MuxTracer => {
                            return Err(Error::UnexpectedConfig(tracer_type));
                        }
                    }
                }
                GethDebugTracerType::JsTracer(ref code) => {
                    let config = match tracer_config {
                        Some(config) => config.into_json(),
                        None => serde_json::Value::Null,
                    };

                    js_tracer = Some(JsInspector::new(code.clone(), config)?);
                }
            }
        }

        let tracing = (!configs.is_empty()).then(|| TracingInspector::new(inspector_config));

        Ok(MuxInspector { four_byte, js_tracer, tracing, configs })
    }

    /// Try converting this [MuxInspector] into a [MuxFrame].
    pub fn try_into_mux_frame<DB: DatabaseRef>(
        &mut self,
        result: &ResultAndState<impl HaltReasonTr>,
        tx: &impl Transaction,
        block: &impl Block,
        db: &DB,
    ) -> Result<MuxFrame, DB::Error> {
        let mut frame = HashMap::with_capacity_and_hasher(self.configs.len(), Default::default());

        let tx_info = TransactionInfo {
            block_number: Some(block.number()),
            base_fee: Some(block.basefee()),
            hash: None,
            block_hash: None,
            index: None,
        };

        for (tracer_type, config) in &self.configs {
            let trace = match config {
                TraceConfig::Call(call_config) => {
                    if let Some(inspector) = &self.tracing {
                        inspector
                            .geth_builder()
                            .geth_call_traces(*call_config, result.result.gas_used())
                            .into()
                    } else {
                        continue;
                    }
                }
                TraceConfig::PreState(prestate_config) => {
                    if let Some(inspector) = &self.tracing {
                        inspector
                            .geth_builder()
                            .geth_prestate_traces(result, prestate_config, db)?
                            .into()
                    } else {
                        continue;
                    }
                }
                TraceConfig::FlatCall(_flatcall_config) => {
                    if let Some(inspector) = &self.tracing {
                        inspector
                            .clone()
                            .into_parity_builder()
                            .into_localized_transaction_traces(tx_info)
                            .into()
                    } else {
                        continue;
                    }
                }
                TraceConfig::Noop => NoopFrame::default().into(),
            };

            frame.insert(GethDebugTracerType::BuiltInTracer(*tracer_type), trace);
        }

        // Add four byte trace if inspector exists
        if let Some(inspector) = &self.four_byte {
            frame.insert(
                GethDebugTracerType::BuiltInTracer(GethDebugBuiltInTracerType::FourByteTracer),
                FourByteFrame::from(inspector).into(),
            );
        }

        // Add js tracer if inspector exists
        if let Some(ref mut js_inspector) = self.js_tracer {
            frame.insert(
                GethDebugTracerType::JsTracer(js_inspector.code().clone()),
                GethTrace::JS(
                    js_inspector
                        .json_result(result, tx, block, db)
                        .unwrap_or_else(|err| serde_json::json!({ "error": err.to_string() })),
                ),
            );
        }

        Ok(MuxFrame(frame))
    }
}

impl<CTX> Inspector<CTX> for MuxInspector
where
    CTX: ContextTr<Journal: JournalExt, Db: DatabaseRef>,
{
    #[inline]
    fn initialize_interp(&mut self, interp: &mut Interpreter, context: &mut CTX) {
        if let Some(ref mut inspector) = self.four_byte {
            inspector.initialize_interp(interp, context);
        }
        if let Some(ref mut inspector) = self.tracing {
            inspector.initialize_interp(interp, context);
        }
        if let Some(ref mut inspector) = self.js_tracer {
            inspector.initialize_interp(interp, context);
        }
    }

    #[inline]
    fn step(&mut self, interp: &mut Interpreter, context: &mut CTX) {
        if let Some(ref mut inspector) = self.four_byte {
            inspector.step(interp, context);
        }
        if let Some(ref mut inspector) = self.tracing {
            inspector.step(interp, context);
        }
        if let Some(ref mut inspector) = self.js_tracer {
            inspector.step(interp, context);
        }
    }

    #[inline]
    fn step_end(&mut self, interp: &mut Interpreter, context: &mut CTX) {
        if let Some(ref mut inspector) = self.four_byte {
            inspector.step_end(interp, context);
        }
        if let Some(ref mut inspector) = self.tracing {
            inspector.step_end(interp, context);
        }
        if let Some(ref mut inspector) = self.js_tracer {
            inspector.step_end(interp, context);
        }
    }

    #[inline]
    fn log(&mut self, interp: &mut Interpreter, context: &mut CTX, log: Log) {
        if let Some(ref mut inspector) = self.four_byte {
            inspector.log(interp, context, log.clone());
        }
        if let Some(ref mut inspector) = self.tracing {
            inspector.log(interp, context, log.clone());
        }
        if let Some(ref mut inspector) = self.js_tracer {
            inspector.log(interp, context, log);
        }
    }

    #[inline]
    fn call(&mut self, context: &mut CTX, inputs: &mut CallInputs) -> Option<CallOutcome> {
        if let Some(ref mut inspector) = self.four_byte {
            let _ = inspector.call(context, inputs);
        }
        if let Some(ref mut inspector) = self.tracing {
            let _ = inspector.call(context, inputs);
        }
        if let Some(ref mut inspector) = self.js_tracer {
            return inspector.call(context, inputs);
        }
        None
    }

    #[inline]
    fn call_end(&mut self, context: &mut CTX, inputs: &CallInputs, outcome: &mut CallOutcome) {
        if let Some(ref mut inspector) = self.four_byte {
            inspector.call_end(context, inputs, outcome);
        }
        if let Some(ref mut inspector) = self.tracing {
            inspector.call_end(context, inputs, outcome);
        }
        if let Some(ref mut inspector) = self.js_tracer {
            inspector.call_end(context, inputs, outcome);
        }
    }

    #[inline]
    fn create(&mut self, context: &mut CTX, inputs: &mut CreateInputs) -> Option<CreateOutcome> {
        if let Some(ref mut inspector) = self.four_byte {
            let _ = inspector.create(context, inputs);
        }
        if let Some(ref mut inspector) = self.tracing {
            let _ = inspector.create(context, inputs);
        }
        if let Some(ref mut inspector) = self.js_tracer {
            return inspector.create(context, inputs);
        }
        None
    }

    #[inline]
    fn create_end(
        &mut self,
        context: &mut CTX,
        inputs: &CreateInputs,
        outcome: &mut CreateOutcome,
    ) {
        if let Some(ref mut inspector) = self.four_byte {
            inspector.create_end(context, inputs, outcome);
        }
        if let Some(ref mut inspector) = self.tracing {
            inspector.create_end(context, inputs, outcome);
        }
        if let Some(ref mut inspector) = self.js_tracer {
            inspector.create_end(context, inputs, outcome);
        }
    }

    #[inline]
    fn eofcreate(
        &mut self,
        context: &mut CTX,
        inputs: &mut EOFCreateInputs,
    ) -> Option<CreateOutcome> {
        if let Some(ref mut inspector) = self.four_byte {
            let _ = inspector.eofcreate(context, inputs);
        }
        if let Some(ref mut inspector) = self.tracing {
            let _ = inspector.eofcreate(context, inputs);
        }
        if let Some(ref mut inspector) = self.js_tracer {
            return inspector.eofcreate(context, inputs);
        }
        None
    }

    #[inline]
    fn eofcreate_end(
        &mut self,
        context: &mut CTX,
        inputs: &EOFCreateInputs,
        outcome: &mut CreateOutcome,
    ) {
        if let Some(ref mut inspector) = self.four_byte {
            inspector.eofcreate_end(context, inputs, outcome);
        }
        if let Some(ref mut inspector) = self.tracing {
            inspector.eofcreate_end(context, inputs, outcome);
        }
        if let Some(ref mut inspector) = self.js_tracer {
            inspector.eofcreate_end(context, inputs, outcome);
        }
    }

    #[inline]
    fn selfdestruct(&mut self, contract: Address, target: Address, value: U256) {
        if let Some(ref mut inspector) = self.four_byte {
            <FourByteInspector as Inspector<CTX>>::selfdestruct(inspector, contract, target, value);
        }
        if let Some(ref mut inspector) = self.tracing {
            <TracingInspector as Inspector<CTX>>::selfdestruct(inspector, contract, target, value);
        }
        if let Some(ref mut inspector) = self.js_tracer {
            <JsInspector as Inspector<CTX>>::selfdestruct(inspector, contract, target, value);
        }
    }
}

/// Error type for [MuxInspector]
#[derive(Debug, Error)]
pub enum Error {
    /// Config was provided for a tracer that does not expect it
    #[error("unexpected config for tracer '{0:?}'")]
    UnexpectedConfig(GethDebugTracerType),
    /// Expected config is missing
    #[error("expected config is missing for tracer '{0:?}'")]
    MissingConfig(GethDebugBuiltInTracerType),
    /// Error when deserializing the config
    #[error("error deserializing config: {0}")]
    InvalidConfig(#[from] serde_json::Error),
    /// Error when creating the JS inspector
    #[error("failed to create JS inspector: {0}")]
    JsInspectorErr(#[from] JsInspectorError),
}
