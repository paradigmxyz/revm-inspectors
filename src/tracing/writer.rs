use super::{
    types::{
        CallKind, CallLog, CallTrace, CallTraceNode, DecodedCallData, DecodedTraceStep,
        TraceMemberOrder,
    },
    CallTraceArena,
};
use alloy_primitives::{address, hex, Address};
use anstyle::{AnsiColor, Color, Style};
use colorchoice::ColorChoice;
use std::io::{self, Write};

const CHEATCODE_ADDRESS: Address = address!("7109709ECfa91a80626fF3989D68f67F5b1DD12D");

const PIPE: &str = "  │ ";
const EDGE: &str = "  └─ ";
const BRANCH: &str = "  ├─ ";
const CALL: &str = "→ ";
const RETURN: &str = "← ";

const TRACE_KIND_STYLE: Style = AnsiColor::Yellow.on_default();
const LOG_STYLE: Style = AnsiColor::Cyan.on_default();

/// Formats [call traces](CallTraceArena) to an [`Write`] writer.
///
/// Will never write invalid UTF-8.
#[derive(Clone, Debug)]
pub struct TraceWriter<W> {
    writer: W,
    use_colors: bool,
    color_cheatcodes: bool,
    indentation_level: u16,
}

impl<W: Write> TraceWriter<W> {
    /// Create a new `TraceWriter` with the given writer.
    #[inline]
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            use_colors: use_colors(ColorChoice::global()),
            color_cheatcodes: false,
            indentation_level: 0,
        }
    }

    /// Sets the color choice.
    #[inline]
    pub fn use_colors(mut self, color_choice: ColorChoice) -> Self {
        self.use_colors = use_colors(color_choice);
        self
    }

    /// Sets whether to color calls to the cheatcode address differently.
    #[inline]
    pub fn color_cheatcodes(mut self, yes: bool) -> Self {
        self.color_cheatcodes = yes;
        self
    }

    /// Sets the starting indentation level.
    #[inline]
    pub fn with_indentation_level(mut self, level: u16) -> Self {
        self.indentation_level = level;
        self
    }

    /// Returns a reference to the inner writer.
    #[inline]
    pub const fn writer(&self) -> &W {
        &self.writer
    }

    /// Returns a mutable reference to the inner writer.
    #[inline]
    pub fn writer_mut(&mut self) -> &mut W {
        &mut self.writer
    }

    /// Consumes the `TraceWriter` and returns the inner writer.
    #[inline]
    pub fn into_writer(self) -> W {
        self.writer
    }

    /// Writes a call trace arena to the writer.
    pub fn write_arena(&mut self, arena: &CallTraceArena) -> io::Result<()> {
        self.write_node(arena.nodes(), 0)?;
        self.writer.flush()
    }

    /// Writes a single item of a single node to the writer. Returns the index of the next item to
    /// be written.
    ///
    /// Note: this will return length of [CallTraceNode::ordering] when last item will get
    /// processed.
    fn write_item(
        &mut self,
        nodes: &[CallTraceNode],
        node_idx: usize,
        item_idx: usize,
    ) -> io::Result<usize> {
        let node = &nodes[node_idx];
        match &node.ordering[item_idx] {
            TraceMemberOrder::Log(index) => {
                self.write_log(&node.logs[*index])?;
                Ok(item_idx + 1)
            }
            TraceMemberOrder::Call(index) => {
                self.write_node(nodes, node.children[*index])?;
                Ok(item_idx + 1)
            }
            TraceMemberOrder::Step(index) => self.write_step(nodes, node_idx, item_idx, *index),
        }
    }

    /// Writes items of a single node to the writer, starting from the given index, and until the
    /// given predicate is false.
    ///
    /// Returns the index of the next item to be written.
    fn write_items_until(
        &mut self,
        nodes: &[CallTraceNode],
        node_idx: usize,
        first_item_idx: usize,
        f: impl Fn(usize) -> bool,
    ) -> io::Result<usize> {
        let mut item_idx = first_item_idx;
        while !f(item_idx) {
            item_idx = self.write_item(nodes, node_idx, item_idx)?;
        }
        Ok(item_idx)
    }

    /// Writes all items of a single node to the writer.
    fn write_items(&mut self, nodes: &[CallTraceNode], node_idx: usize) -> io::Result<()> {
        let items_cnt = nodes[node_idx].ordering.len();
        self.write_items_until(nodes, node_idx, 0, |idx| idx == items_cnt)?;
        Ok(())
    }

    /// Writes a single node and its children to the writer.
    fn write_node(&mut self, nodes: &[CallTraceNode], idx: usize) -> io::Result<()> {
        let node = &nodes[idx];

        // Write header.
        self.write_branch()?;
        self.write_trace_header(&node.trace)?;
        self.writer.write_all(b"\n")?;

        // Write logs and subcalls.
        self.indentation_level += 1;
        self.write_items(nodes, idx)?;

        // Write return data.
        self.write_edge()?;
        self.write_trace_footer(&node.trace)?;
        self.writer.write_all(b"\n")?;

        self.indentation_level -= 1;

        Ok(())
    }

    /// Writes the header of a call trace.
    fn write_trace_header(&mut self, trace: &CallTrace) -> io::Result<()> {
        write!(self.writer, "[{}] ", trace.gas_used)?;

        let trace_kind_style = self.trace_kind_style();
        let address = trace.address.to_checksum_buffer(None);

        if trace.kind.is_any_create() {
            write!(
                self.writer,
                "{trace_kind_style}{CALL}new{trace_kind_style:#} {label}@{address}",
                label = trace.decoded.label.as_deref().unwrap_or("<unknown>")
            )?;
        } else {
            let (func_name, inputs) = match &trace.decoded.call_data {
                Some(DecodedCallData { signature, args }) => {
                    let name = signature.split('(').next().unwrap();
                    (name.to_string(), args.join(", "))
                }
                None => {
                    if trace.data.len() < 4 {
                        ("fallback".to_string(), hex::encode(&trace.data))
                    } else {
                        let (selector, data) = trace.data.split_at(4);
                        (hex::encode(selector), hex::encode(data))
                    }
                }
            };

            write!(
                self.writer,
                "{style}{addr}{style:#}::{style}{func_name}{style:#}",
                style = self.trace_style(trace),
                addr = trace.decoded.label.as_deref().unwrap_or(address.as_str()),
            )?;

            if !trace.value.is_zero() {
                write!(self.writer, "{{value: {}}}", trace.value)?;
            }

            write!(self.writer, "({inputs})")?;

            let action = match trace.kind {
                CallKind::Call => None,
                CallKind::StaticCall => Some(" [staticcall]"),
                CallKind::CallCode => Some(" [callcode]"),
                CallKind::DelegateCall => Some(" [delegatecall]"),
                CallKind::AuthCall => Some(" [authcall]"),
                CallKind::Create | CallKind::Create2 | CallKind::EOFCreate => unreachable!(),
            };
            if let Some(action) = action {
                write!(self.writer, "{trace_kind_style}{action}{trace_kind_style:#}")?;
            }
        }

        Ok(())
    }

    fn write_log(&mut self, log: &CallLog) -> io::Result<()> {
        let log_style = self.log_style();
        self.write_branch()?;

        if let Some(name) = &log.decoded.name {
            write!(self.writer, "emit {name}({log_style}")?;
            if let Some(params) = &log.decoded.params {
                for (i, (param_name, value)) in params.iter().enumerate() {
                    if i > 0 {
                        self.writer.write_all(b", ")?;
                    }
                    write!(self.writer, "{param_name}: {value}")?;
                }
            }
            writeln!(self.writer, "{log_style:#})")?;
        } else {
            for (i, topic) in log.raw_log.topics().iter().enumerate() {
                if i == 0 {
                    self.writer.write_all(b" emit topic 0")?;
                } else {
                    self.write_pipes()?;
                    write!(self.writer, "       topic {i}")?;
                }
                writeln!(self.writer, ": {log_style}{topic}{log_style:#}")?;
            }

            if !log.raw_log.topics().is_empty() {
                self.write_pipes()?;
            }
            writeln!(
                self.writer,
                "          data: {log_style}{data}{log_style:#}",
                data = log.raw_log.data
            )?;
        }

        Ok(())
    }

    /// Writes a single step of a single node to the writer. Returns the index of the next item to
    /// be written.
    fn write_step(
        &mut self,
        nodes: &[CallTraceNode],
        node_idx: usize,
        item_idx: usize,
        step_idx: usize,
    ) -> io::Result<usize> {
        let node = &nodes[node_idx];
        let step = &node.trace.steps[step_idx];

        let Some(decoded) = &step.decoded else {
            // We only write explicitly decoded steps to avoid bloating the output.
            return Ok(item_idx + 1);
        };

        match decoded {
            DecodedTraceStep::InternalCall(call, end_idx) => {
                let gas_used = node.trace.steps[*end_idx].gas_used.saturating_sub(step.gas_used);
                self.write_branch()?;
                self.indentation_level += 1;

                writeln!(
                    self.writer,
                    "[{}] {}{}",
                    gas_used,
                    call.func_name,
                    call.args.as_ref().map(|v| format!("({})", v.join(", "))).unwrap_or_default()
                )?;

                let end_item_idx =
                    self.write_items_until(nodes, node_idx, item_idx + 1, |item_idx: usize| {
                        matches!(&node.ordering[item_idx], TraceMemberOrder::Step(idx) if *idx == *end_idx)
                    })?;

                self.write_edge()?;
                write!(self.writer, "{RETURN}")?;

                if let Some(outputs) = &call.return_data {
                    write!(self.writer, "{}", outputs.join(", "))?;
                }

                writeln!(self.writer)?;

                self.indentation_level -= 1;

                Ok(end_item_idx + 1)
            }
            DecodedTraceStep::Line(line) => {
                self.write_branch()?;
                writeln!(self.writer, "{line}")?;

                Ok(item_idx + 1)
            }
        }
    }

    /// Writes the footer of a call trace.
    fn write_trace_footer(&mut self, trace: &CallTrace) -> io::Result<()> {
        write!(
            self.writer,
            "{style}{RETURN}[{status:?}] {style:#}",
            style = self.trace_style(trace),
            status = trace.status,
        )?;

        if let Some(decoded) = &trace.decoded.return_data {
            return self.writer.write_all(decoded.as_bytes());
        }

        if trace.kind.is_any_create() {
            write!(self.writer, "{} bytes of code", trace.output.len())?;
        } else if !trace.output.is_empty() {
            write!(self.writer, "{}", trace.output)?;
        }

        Ok(())
    }

    fn write_indentation(&mut self) -> io::Result<()> {
        self.writer.write_all(b"  ")?;
        for _ in 1..self.indentation_level {
            self.writer.write_all(PIPE.as_bytes())?;
        }
        Ok(())
    }

    #[doc(alias = "left_prefix")]
    fn write_branch(&mut self) -> io::Result<()> {
        self.write_indentation()?;
        if self.indentation_level != 0 {
            self.writer.write_all(BRANCH.as_bytes())?;
        }
        Ok(())
    }

    #[doc(alias = "right_prefix")]
    fn write_pipes(&mut self) -> io::Result<()> {
        self.write_indentation()?;
        self.writer.write_all(PIPE.as_bytes())
    }

    fn write_edge(&mut self) -> io::Result<()> {
        self.write_indentation()?;
        self.writer.write_all(EDGE.as_bytes())
    }

    fn trace_style(&self, trace: &CallTrace) -> Style {
        if !self.use_colors {
            return Style::default();
        }
        let color = if self.color_cheatcodes && trace.address == CHEATCODE_ADDRESS {
            AnsiColor::Blue
        } else if trace.success {
            AnsiColor::Green
        } else {
            AnsiColor::Red
        };
        Color::Ansi(color).on_default()
    }

    fn trace_kind_style(&self) -> Style {
        if !self.use_colors {
            return Style::default();
        }
        TRACE_KIND_STYLE
    }

    fn log_style(&self) -> Style {
        if !self.use_colors {
            return Style::default();
        }
        LOG_STYLE
    }
}

fn use_colors(choice: ColorChoice) -> bool {
    use io::IsTerminal;
    match choice {
        ColorChoice::Auto => io::stdout().is_terminal(),
        ColorChoice::AlwaysAnsi | ColorChoice::Always => true,
        ColorChoice::Never => false,
    }
}
