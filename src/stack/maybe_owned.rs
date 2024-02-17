use alloy_primitives::{Log, U256};
use revm::{
    interpreter::{CallInputs, CallOutcome, CreateInputs, CreateOutcome, Interpreter},
    primitives::{db::Database, Address},
    EvmContext, Inspector,
};
use std::{
    cell::{Ref, RefCell},
    rc::Rc,
};

/// An [Inspector] that is either owned by an individual [Inspector] or is shared as part of a
/// series of inspectors in a [InspectorStack](crate::stack::InspectorStack).
///
/// Caution: if the [Inspector] is _stacked_ then it _must_ be called first.
#[derive(Debug)]
pub enum MaybeOwnedInspector<I> {
    /// Inspector is owned.
    Owned(Rc<RefCell<I>>),
    /// Inspector is shared and part of a stack
    Stacked(Rc<RefCell<I>>),
}

impl<I> MaybeOwnedInspector<I> {
    /// Create a new _owned_ instance
    pub fn new_owned(inspector: I) -> Self {
        Self::Owned(Rc::new(RefCell::new(inspector)))
    }

    /// Creates a [MaybeOwnedInspector::Stacked] clone of this type.
    pub fn clone_stacked(&self) -> Self {
        match self {
            Self::Owned(gas) | Self::Stacked(gas) => Self::Stacked(Rc::clone(gas)),
        }
    }

    /// Returns a reference to the inspector.
    pub fn as_ref(&self) -> Ref<'_, I> {
        match self {
            Self::Owned(insp) => insp.borrow(),
            Self::Stacked(insp) => insp.borrow(),
        }
    }
}

impl<I: Default> MaybeOwnedInspector<I> {
    /// Create a new _owned_ instance
    pub fn owned() -> Self {
        Self::new_owned(Default::default())
    }
}

impl<I: Default> Default for MaybeOwnedInspector<I> {
    fn default() -> Self {
        Self::owned()
    }
}

impl<I> Clone for MaybeOwnedInspector<I> {
    fn clone(&self) -> Self {
        self.clone_stacked()
    }
}

impl<I, DB> Inspector<DB> for MaybeOwnedInspector<I>
where
    DB: Database,
    I: Inspector<DB>,
{
    fn initialize_interp(&mut self, interp: &mut Interpreter, context: &mut EvmContext<DB>) {
        match self {
            Self::Owned(insp) => insp.borrow_mut().initialize_interp(interp, context),
            Self::Stacked(_) => {}
        }
    }

    fn step(&mut self, interp: &mut Interpreter, context: &mut EvmContext<DB>) {
        match self {
            Self::Owned(insp) => insp.borrow_mut().step(interp, context),
            Self::Stacked(_) => {}
        }
    }

    fn step_end(&mut self, interp: &mut Interpreter, context: &mut EvmContext<DB>) {
        match self {
            Self::Owned(insp) => insp.borrow_mut().step_end(interp, context),
            Self::Stacked(_) => {}
        }
    }

    fn log(&mut self, context: &mut EvmContext<DB>, log: &Log) {
        match self {
            Self::Owned(insp) => insp.borrow_mut().log(context, log),
            Self::Stacked(_) => {}
        }
    }

    fn call(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &mut CallInputs,
    ) -> Option<CallOutcome> {
        match self {
            Self::Owned(insp) => insp.borrow_mut().call(context, inputs),
            Self::Stacked(_) => None,
        }
    }

    fn call_end(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &CallInputs,
        outcome: CallOutcome,
    ) -> CallOutcome {
        match self {
            Self::Owned(insp) => insp.borrow_mut().call_end(context, inputs, outcome),
            Self::Stacked(_) => outcome,
        }
    }

    fn create(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &mut CreateInputs,
    ) -> Option<CreateOutcome> {
        match self {
            Self::Owned(insp) => insp.borrow_mut().create(context, inputs),
            Self::Stacked(_) => None,
        }
    }

    fn create_end(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &CreateInputs,
        outcome: CreateOutcome,
    ) -> CreateOutcome {
        match self {
            Self::Owned(insp) => insp.borrow_mut().create_end(context, inputs, outcome),
            Self::Stacked(_) => outcome,
        }
    }

    fn selfdestruct(&mut self, contract: Address, target: Address, value: U256) {
        match self {
            Self::Owned(insp) => insp.borrow_mut().selfdestruct(contract, target, value),
            Self::Stacked(_) => {}
        }
    }
}
