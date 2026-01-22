use crate::tracing::{FourByteInspector, TracingInspector, TracingInspectorConfig};
use alloc::vec::Vec;
use alloy_primitives::{map::HashMap, Address, Log, U256};
use alloy_rpc_types_eth::TransactionInfo;
use alloy_rpc_types_trace::geth::{
    mux::{MuxConfig, MuxFrame},
    CallConfig, FlatCallConfig, FourByteFrame, GethDebugBuiltInTracerType, NoopFrame,
    PreStateConfig,
};
#[cfg(feature = "js-tracer")]
use alloy_rpc_types_trace::geth::{GethDebugTracerConfig, GethDebugTracerType, GethTrace};
#[cfg(feature = "js-tracer")]
use revm::context::{Block, Transaction};
use revm::{
    context_interface::{
        result::{HaltReasonTr, ResultAndState},
        ContextTr,
    },
    inspector::JournalExt,
    interpreter::{CallInputs, CallOutcome, CreateInputs, CreateOutcome, Interpreter},
    DatabaseRef, Inspector,
};
use thiserror::Error;

#[cfg(feature = "js-tracer")]
use crate::tracing::js::{JsInspector, JsInspectorError};

/// Mux tracing inspector that runs and collects results of multiple inspectors at once.
#[derive(Debug)]
pub struct MuxInspector {
    /// An instance of FourByteInspector that can be reused
    four_byte: Option<FourByteInspector>,
    /// An instance of JsInspector that can be reused
    #[cfg(feature = "js-tracer")]
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

/// Extended mux configuration that supports JS tracers.
///
/// This wraps a HashMap of `GethDebugTracerType` to optional configs,
/// allowing both built-in tracers and JS tracers to be configured.
#[cfg(feature = "js-tracer")]
#[derive(Clone, Debug, Default)]
pub struct MuxConfigExt(pub HashMap<GethDebugTracerType, Option<GethDebugTracerConfig>>);

impl MuxInspector {
    /// Try creating a new instance of [MuxInspector] from the given [MuxConfig].
    ///
    /// Note: This only supports built-in tracers. For JS tracer support, use
    /// [`try_from_config_ext`](Self::try_from_config_ext).
    pub fn try_from_config(config: MuxConfig) -> Result<MuxInspector, Error> {
        let mut four_byte = None;
        let mut inspector_config = TracingInspectorConfig::none();
        let mut configs = Vec::new();

        // Process each tracer configuration
        for (tracer_type, tracer_config) in config.0 {
            #[allow(unreachable_patterns)]
            match tracer_type {
                GethDebugBuiltInTracerType::FourByteTracer => {
                    if tracer_config.is_some() {
                        return Err(Error::UnexpectedBuiltInConfig(tracer_type));
                    }
                    four_byte = Some(FourByteInspector::default());
                }
                GethDebugBuiltInTracerType::CallTracer => {
                    let call_config = tracer_config
                        .ok_or(Error::MissingConfig(tracer_type))?
                        .into_call_config()?;

                    inspector_config
                        .merge(TracingInspectorConfig::from_geth_call_config(&call_config));
                    configs.push((tracer_type, TraceConfig::Call(call_config)));
                }
                GethDebugBuiltInTracerType::PreStateTracer => {
                    let prestate_config = tracer_config
                        .ok_or(Error::MissingConfig(tracer_type))?
                        .into_pre_state_config()?;

                    inspector_config
                        .merge(TracingInspectorConfig::from_geth_prestate_config(&prestate_config));
                    configs.push((tracer_type, TraceConfig::PreState(prestate_config)));
                }
                GethDebugBuiltInTracerType::NoopTracer => {
                    if tracer_config.is_some() {
                        return Err(Error::UnexpectedBuiltInConfig(tracer_type));
                    }
                    configs.push((tracer_type, TraceConfig::Noop));
                }
                GethDebugBuiltInTracerType::FlatCallTracer => {
                    let flatcall_config = tracer_config
                        .ok_or(Error::MissingConfig(tracer_type))?
                        .into_flat_call_config()?;

                    inspector_config
                        .merge(TracingInspectorConfig::from_flat_call_config(&flatcall_config));
                    configs.push((tracer_type, TraceConfig::FlatCall(flatcall_config)));
                }
                GethDebugBuiltInTracerType::MuxTracer => {
                    return Err(Error::UnexpectedBuiltInConfig(tracer_type));
                }
                _ => {
                    // keep this so that new variants can be supported
                    return Err(Error::UnexpectedBuiltInConfig(tracer_type));
                }
            }
        }

        let tracing = (!configs.is_empty()).then(|| TracingInspector::new(inspector_config));

        Ok(MuxInspector {
            four_byte,
            #[cfg(feature = "js-tracer")]
            js_tracer: None,
            tracing,
            configs,
        })
    }

    /// Try creating a new instance of [MuxInspector] from the given extended [MuxConfigExt].
    ///
    /// This supports both built-in tracers and JS tracers.
    #[cfg(feature = "js-tracer")]
    pub fn try_from_config_ext(config: MuxConfigExt) -> Result<MuxInspector, Error> {
        let mut four_byte = None;
        let mut js_tracer: Option<JsInspector> = None;
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
                                .ok_or(Error::MissingBuiltInConfig(built_in_tracer_type))?
                                .into_call_config()?;

                            inspector_config
                                .merge(TracingInspectorConfig::from_geth_call_config(&call_config));
                            configs.push((built_in_tracer_type, TraceConfig::Call(call_config)));
                        }
                        GethDebugBuiltInTracerType::PreStateTracer => {
                            let prestate_config = tracer_config
                                .ok_or(Error::MissingBuiltInConfig(built_in_tracer_type))?
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
                                .ok_or(Error::MissingBuiltInConfig(built_in_tracer_type))?
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
                        _ => {
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
    ///
    /// Note: This only returns built-in tracer results. For JS tracer support, use
    /// [`try_into_mux_frame_ext`](Self::try_into_mux_frame_ext).
    pub fn try_into_mux_frame<DB: DatabaseRef>(
        &self,
        result: &ResultAndState<impl HaltReasonTr>,
        db: &DB,
        tx_info: TransactionInfo,
    ) -> Result<MuxFrame, DB::Error> {
        let mut frame = HashMap::with_capacity_and_hasher(self.configs.len(), Default::default());

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

            frame.insert(*tracer_type, trace);
        }

        // Add four byte trace if inspector exists
        if let Some(inspector) = &self.four_byte {
            frame.insert(
                GethDebugBuiltInTracerType::FourByteTracer,
                FourByteFrame::from(inspector).into(),
            );
        }

        Ok(MuxFrame(frame))
    }

    /// Try converting this [MuxInspector] into an extended mux frame.
    ///
    /// Returns a tuple of `(MuxFrame, Option<GethTrace>)` where:
    /// - `MuxFrame` contains all built-in tracer results
    /// - `Option<GethTrace>` contains the JS tracer result if configured
    #[cfg(feature = "js-tracer")]
    pub fn try_into_mux_frame_ext<DB: DatabaseRef>(
        &mut self,
        result: &ResultAndState<impl HaltReasonTr>,
        tx: &impl Transaction,
        block: &impl Block,
        db: &DB,
        tx_info: TransactionInfo,
    ) -> Result<(MuxFrame, Option<GethTrace>), DB::Error> {
        let mux_frame = self.try_into_mux_frame(result, db, tx_info)?;

        // Get js tracer result if inspector exists
        let js_result = if let Some(ref mut js_inspector) = self.js_tracer {
            Some(GethTrace::JS(
                js_inspector
                    .json_result(result, tx, block, db)
                    .unwrap_or_else(|err| serde_json::json!({ "error": err.to_string() })),
            ))
        } else {
            None
        };

        Ok((mux_frame, js_result))
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
        #[cfg(feature = "js-tracer")]
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
        #[cfg(feature = "js-tracer")]
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
        #[cfg(feature = "js-tracer")]
        if let Some(ref mut inspector) = self.js_tracer {
            inspector.step_end(interp, context);
        }
    }

    #[inline]
    fn log(&mut self, context: &mut CTX, log: Log) {
        if let Some(ref mut inspector) = self.four_byte {
            inspector.log(context, log.clone());
        }
        #[cfg(feature = "js-tracer")]
        if let Some(ref mut inspector) = self.js_tracer {
            inspector.log(context, log.clone());
        }
        if let Some(ref mut inspector) = self.tracing {
            inspector.log(context, log);
        }
    }

    #[inline]
    fn log_full(&mut self, interp: &mut Interpreter, context: &mut CTX, log: Log) {
        if let Some(ref mut inspector) = self.four_byte {
            inspector.log_full(interp, context, log.clone());
        }
        #[cfg(feature = "js-tracer")]
        if let Some(ref mut inspector) = self.js_tracer {
            inspector.log_full(interp, context, log.clone());
        }
        if let Some(ref mut inspector) = self.tracing {
            inspector.log_full(interp, context, log);
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
        #[cfg(feature = "js-tracer")]
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
        #[cfg(feature = "js-tracer")]
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
        #[cfg(feature = "js-tracer")]
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
        #[cfg(feature = "js-tracer")]
        if let Some(ref mut inspector) = self.js_tracer {
            inspector.create_end(context, inputs, outcome);
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
        #[cfg(feature = "js-tracer")]
        if let Some(ref mut inspector) = self.js_tracer {
            <JsInspector as Inspector<CTX>>::selfdestruct(inspector, contract, target, value);
        }
    }
}

/// Error type for [MuxInspector]
#[derive(Debug, Error)]
pub enum Error {
    /// Config was provided for a tracer that does not expect it
    #[cfg(feature = "js-tracer")]
    #[error("unexpected config for tracer '{0:?}'")]
    UnexpectedConfig(GethDebugTracerType),
    /// Config was provided for a built-in tracer that does not expect it
    #[error("unexpected config for tracer '{0:?}'")]
    UnexpectedBuiltInConfig(GethDebugBuiltInTracerType),
    /// Expected config is missing
    #[error("expected config is missing for tracer '{0:?}'")]
    MissingConfig(GethDebugBuiltInTracerType),
    /// Expected config is missing for built-in tracer
    #[cfg(feature = "js-tracer")]
    #[error("expected config is missing for tracer '{0:?}'")]
    MissingBuiltInConfig(GethDebugBuiltInTracerType),
    /// Error when deserializing the config
    #[error("error deserializing config: {0}")]
    InvalidConfig(#[from] serde_json::Error),
    /// Error when creating the JS inspector
    #[cfg(feature = "js-tracer")]
    #[error("failed to create JS inspector: {0}")]
    JsInspectorErr(#[from] JsInspectorError),
}
