use crate::tracing::{FourByteInspector, TracingInspector, TracingInspectorConfig};
use alloy_primitives::{Address, Log, U256};
use alloy_rpc_trace_types::geth::{
    mux::{MuxConfig, MuxFrame},
    CallConfig, FourByteFrame, GethDebugBuiltInTracerType, GethDebugTracerConfig, GethTrace,
    NoopFrame, PreStateConfig,
};
use revm::{
    interpreter::{CallInputs, CallOutcome, CreateInputs, CreateOutcome, Interpreter},
    primitives::ResultAndState,
    Database, DatabaseRef, EvmContext, Inspector,
};
use std::collections::HashMap;
use thiserror::Error;

/// Mux tracing inspector that runs and collects results of multiple inspectors at once.
#[derive(Clone, Debug)]
pub struct MuxInspector {
    /// The list of tracer types with its inspectors.
    inspectors: Vec<(GethDebugBuiltInTracerType, MultiInspector)>,
}

impl MuxInspector {
    /// Try creating a new instance of [MuxInspector] from the given [MuxConfig].
    pub fn try_from_config(config: MuxConfig) -> Result<MuxInspector, Error> {
        let inspectors = config
            .tracers
            .into_iter()
            .map(|(tracer_type, tracer_config)| {
                MultiInspector::try_from_config(tracer_type, tracer_config.clone())
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(MuxInspector { inspectors })
    }

    /// Try converting this [MuxInspector] into a [MuxFrame].
    pub fn try_into_mux_frame<DB: DatabaseRef>(
        self,
        result: &ResultAndState,
        db: &DB,
    ) -> Result<MuxFrame, DB::Error> {
        let mut frame = HashMap::with_capacity(self.inspectors.len());
        for (tracer_type, inspector) in self.inspectors {
            let trace = match inspector {
                MultiInspector::FourByte(inspector) => FourByteFrame::from(inspector).into(),
                MultiInspector::Call(config, inspector) => inspector
                    .into_geth_builder()
                    .geth_call_traces(config, result.result.gas_used())
                    .into(),
                MultiInspector::Prestate(config, inspector) => {
                    inspector.into_geth_builder().geth_prestate_traces(result, config, db)?.into()
                }
                MultiInspector::Noop => NoopFrame::default().into(),
                MultiInspector::Mux(inspector) => {
                    inspector.try_into_mux_frame(result, db).map(GethTrace::MuxTracer)?
                }
            };

            frame.insert(tracer_type, trace);
        }

        Ok(MuxFrame(frame))
    }
}

impl<DB> Inspector<DB> for MuxInspector
where
    DB: Database,
{
    #[inline]
    fn initialize_interp(&mut self, interp: &mut Interpreter, context: &mut EvmContext<DB>) {
        for (_, inspector) in &mut self.inspectors {
            inspector.initialize_interp(interp, context);
        }
    }

    #[inline]
    fn step(&mut self, interp: &mut Interpreter, context: &mut EvmContext<DB>) {
        for (_, inspector) in &mut self.inspectors {
            inspector.step(interp, context);
        }
    }

    #[inline]
    fn step_end(&mut self, interp: &mut Interpreter, context: &mut EvmContext<DB>) {
        for (_, inspector) in &mut self.inspectors {
            inspector.step_end(interp, context);
        }
    }

    #[inline]
    fn log(&mut self, context: &mut EvmContext<DB>, log: &Log) {
        for (_, inspector) in &mut self.inspectors {
            inspector.log(context, log);
        }
    }

    #[inline]
    fn call(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &mut CallInputs,
    ) -> Option<CallOutcome> {
        for (_, inspector) in &mut self.inspectors {
            if let Some(outcome) = inspector.call(context, inputs) {
                return Some(outcome);
            }
        }

        None
    }

    #[inline]
    fn call_end(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &CallInputs,
        outcome: CallOutcome,
    ) -> CallOutcome {
        let mut outcome = outcome;
        for (_, inspector) in &mut self.inspectors {
            outcome = inspector.call_end(context, inputs, outcome);
        }

        outcome
    }

    #[inline]
    fn create(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &mut CreateInputs,
    ) -> Option<CreateOutcome> {
        for (_, inspector) in &mut self.inspectors {
            if let Some(outcome) = inspector.create(context, inputs) {
                return Some(outcome);
            }
        }

        None
    }

    #[inline]
    fn create_end(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &CreateInputs,
        outcome: CreateOutcome,
    ) -> CreateOutcome {
        let mut outcome = outcome;
        for (_, inspector) in &mut self.inspectors {
            outcome = inspector.create_end(context, inputs, outcome);
        }

        outcome
    }

    #[inline]
    fn selfdestruct(&mut self, contract: Address, target: Address, value: U256) {
        for (_, inspector) in &mut self.inspectors {
            inspector.selfdestruct::<DB>(contract, target, value);
        }
    }
}

/// An inspector that can delegate to multiple inspector types.
#[derive(Clone, Debug)]
enum MultiInspector {
    FourByte(FourByteInspector),
    Call(CallConfig, TracingInspector),
    Prestate(PreStateConfig, TracingInspector),
    Noop,
    Mux(MuxInspector),
}

impl MultiInspector {
    /// Try creating a new instance of [MultiInspector] from the given tracer type and config.
    pub(crate) fn try_from_config(
        tracer_type: GethDebugBuiltInTracerType,
        tracer_config: Option<GethDebugTracerConfig>,
    ) -> Result<(GethDebugBuiltInTracerType, MultiInspector), Error> {
        let inspector = match tracer_type {
            GethDebugBuiltInTracerType::FourByteTracer => {
                if tracer_config.is_some() {
                    return Err(Error::UnexpectedConfig(tracer_type));
                }
                Ok(MultiInspector::FourByte(FourByteInspector::default()))
            }
            GethDebugBuiltInTracerType::CallTracer => {
                let call_config = tracer_config
                    .ok_or_else(|| Error::MissingConfig(tracer_type))?
                    .into_call_config()?;

                let inspector = TracingInspector::new(
                    TracingInspectorConfig::default_geth()
                        .set_record_logs(call_config.with_log.unwrap_or_default()),
                );

                Ok(MultiInspector::Call(call_config, inspector))
            }
            GethDebugBuiltInTracerType::PreStateTracer => {
                let prestate_config = tracer_config
                    .ok_or_else(|| Error::MissingConfig(tracer_type))?
                    .into_pre_state_config()?;

                let inspector = TracingInspector::new(
                    TracingInspectorConfig::default_geth()
                        // if in default mode, we need to return all touched storages, for
                        // which we need to record steps and statediff
                        .set_steps_and_state_diffs(prestate_config.is_default_mode()),
                );

                Ok(MultiInspector::Prestate(prestate_config, inspector))
            }
            GethDebugBuiltInTracerType::NoopTracer => {
                if tracer_config.is_some() {
                    return Err(Error::UnexpectedConfig(tracer_type));
                }
                Ok(MultiInspector::Noop)
            }
            GethDebugBuiltInTracerType::MuxTracer => {
                let config = tracer_config
                    .ok_or_else(|| Error::MissingConfig(tracer_type))?
                    .into_mux_config()?;

                Ok(MultiInspector::Mux(MuxInspector::try_from_config(config)?))
            }
        };

        inspector.map(|inspector| (tracer_type, inspector))
    }

    #[inline]
    fn initialize_interp<DB: Database>(
        &mut self,
        interp: &mut Interpreter,
        context: &mut EvmContext<DB>,
    ) {
        match self {
            MultiInspector::FourByte(inspector) => inspector.initialize_interp(interp, context),
            MultiInspector::Call(_, inspector) => inspector.initialize_interp(interp, context),
            MultiInspector::Prestate(_, inspector) => inspector.initialize_interp(interp, context),
            MultiInspector::Noop => {}
            MultiInspector::Mux(inspector) => inspector.initialize_interp(interp, context),
        }
    }

    #[inline]
    fn step<DB: Database>(&mut self, interp: &mut Interpreter, context: &mut EvmContext<DB>) {
        match self {
            MultiInspector::FourByte(inspector) => inspector.step(interp, context),
            MultiInspector::Call(_, inspector) => inspector.step(interp, context),
            MultiInspector::Prestate(_, inspector) => inspector.step(interp, context),
            MultiInspector::Noop => {}
            MultiInspector::Mux(inspector) => inspector.step(interp, context),
        }
    }

    #[inline]
    fn step_end<DB: Database>(&mut self, interp: &mut Interpreter, context: &mut EvmContext<DB>) {
        match self {
            MultiInspector::FourByte(inspector) => inspector.step_end(interp, context),
            MultiInspector::Call(_, inspector) => inspector.step_end(interp, context),
            MultiInspector::Prestate(_, inspector) => inspector.step_end(interp, context),
            MultiInspector::Noop => {}
            MultiInspector::Mux(inspector) => inspector.step_end(interp, context),
        }
    }

    #[inline]
    fn log<DB: Database>(&mut self, context: &mut EvmContext<DB>, log: &Log) {
        match self {
            MultiInspector::FourByte(inspector) => inspector.log(context, log),
            MultiInspector::Call(_, inspector) => inspector.log(context, log),
            MultiInspector::Prestate(_, inspector) => inspector.log(context, log),
            MultiInspector::Noop => {}
            MultiInspector::Mux(inspector) => inspector.log(context, log),
        }
    }

    #[inline]
    fn call<DB: Database>(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &mut CallInputs,
    ) -> Option<CallOutcome> {
        match self {
            MultiInspector::FourByte(inspector) => inspector.call(context, inputs),
            MultiInspector::Call(_, inspector) => inspector.call(context, inputs),
            MultiInspector::Prestate(_, inspector) => inspector.call(context, inputs),
            MultiInspector::Noop => None,
            MultiInspector::Mux(inspector) => inspector.call(context, inputs),
        };

        None
    }

    #[inline]
    fn call_end<DB: Database>(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &CallInputs,
        outcome: CallOutcome,
    ) -> CallOutcome {
        match self {
            MultiInspector::FourByte(inspector) => inspector.call_end(context, inputs, outcome),
            MultiInspector::Call(_, inspector) => inspector.call_end(context, inputs, outcome),
            MultiInspector::Prestate(_, inspector) => inspector.call_end(context, inputs, outcome),
            MultiInspector::Noop => outcome,
            MultiInspector::Mux(inspector) => inspector.call_end(context, inputs, outcome),
        }
    }

    #[inline]
    fn create<DB: Database>(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &mut CreateInputs,
    ) -> Option<CreateOutcome> {
        match self {
            MultiInspector::FourByte(inspector) => inspector.create(context, inputs),
            MultiInspector::Call(_, inspector) => inspector.create(context, inputs),
            MultiInspector::Prestate(_, inspector) => inspector.create(context, inputs),
            MultiInspector::Noop => None,
            MultiInspector::Mux(inspector) => inspector.create(context, inputs),
        };

        None
    }

    #[inline]
    fn create_end<DB: Database>(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &CreateInputs,
        outcome: CreateOutcome,
    ) -> CreateOutcome {
        match self {
            MultiInspector::FourByte(inspector) => inspector.create_end(context, inputs, outcome),
            MultiInspector::Call(_, inspector) => inspector.create_end(context, inputs, outcome),
            MultiInspector::Prestate(_, inspector) => {
                inspector.create_end(context, inputs, outcome)
            }
            MultiInspector::Noop => outcome,
            MultiInspector::Mux(inspector) => inspector.create_end(context, inputs, outcome),
        }
    }

    #[inline]
    fn selfdestruct<DB: Database>(&mut self, contract: Address, target: Address, value: U256) {
        match self {
            MultiInspector::FourByte(inspector) => {
                <FourByteInspector as Inspector<DB>>::selfdestruct(
                    inspector, contract, target, value,
                )
            }
            MultiInspector::Call(_, inspector) => {
                <TracingInspector as Inspector<DB>>::selfdestruct(
                    inspector, contract, target, value,
                )
            }
            MultiInspector::Prestate(_, inspector) => {
                <TracingInspector as Inspector<DB>>::selfdestruct(
                    inspector, contract, target, value,
                )
            }
            MultiInspector::Noop => {}
            MultiInspector::Mux(inspector) => {
                <MuxInspector as Inspector<DB>>::selfdestruct(inspector, contract, target, value)
            }
        }
    }
}

/// Error type for [MuxInspector]
#[derive(Debug, Error)]
pub enum Error {
    /// Config was provided for a tracer that does not expect it
    #[error("unexpected config for tracer '{0:?}'")]
    UnexpectedConfig(GethDebugBuiltInTracerType),
    /// Expected config is missing
    #[error("expected config is missing for tracer '{0:?}'")]
    MissingConfig(GethDebugBuiltInTracerType),
    /// Error when deserializing the config
    #[error("error deserializing config: {0}")]
    InvalidConfig(#[from] serde_json::Error),
}
