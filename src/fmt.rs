use crate::opts::Opts;

use chrono::Utc;
use colored::Colorize;
use std::env;
use std::fmt;
use tracing::{field::Field, info_span, trace, Level};
use tracing_core::{Event, Subscriber};
use tracing_subscriber::field::{MakeExt, MakeVisitor, RecordFields, VisitFmt};
use tracing_subscriber::field::{Visit, VisitOutput};
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields, FormattedFields};
use tracing_subscriber::registry::LookupSpan;

static DEFAULT_FIELD_DELIM: &str = ", ";

#[derive(Debug)]
struct FmtFilter<'delim> {
    hide_date: bool,
    hide_fields: bool,
    hide_level: bool,
    hide_spans: bool,
    delimiter: &'delim str,
}

impl<'delim> Default for FmtFilter<'delim> {
    fn default() -> Self {
        Self {
            hide_date: false,
            hide_fields: false,
            hide_level: false,
            hide_spans: false,
            delimiter: DEFAULT_FIELD_DELIM,
        }
    }
}

impl<'delim> From<String> for FmtFilter<'delim> {
    fn from(filter_string: String) -> Self {
        FmtFilter::from(filter_string.as_str())
    }
}

impl<'a, 'delim> From<&'a str> for FmtFilter<'delim> {
    fn from(filter_str: &'a str) -> Self {
        let mut filter = Self::default();

        filter_str
            .chars()
            .for_each(|c| match c.to_ascii_lowercase() {
                'd' => filter.hide_date = true,
                'f' => filter.hide_fields = true,
                'l' => filter.hide_level = true,
                's' => filter.hide_spans = true,
                _ => {}
            });

        filter
    }
}

pub fn setup_tracing(opts: &Opts) {
    let span = info_span!("setup-tracing");
    let _enter = span.enter();

    let filter = if let Some(filter) = env::var_os("RUST_LOG") {
        if opts.quiet {
            "".to_string()
        } else {
            filter.to_string_lossy().to_string()
        }
    } else if opts.quiet {
        "pkger=error".to_string()
    } else if opts.debug {
        "pkger=trace".to_string()
    } else {
        "pkger=info".to_string()
    };

    let fmt_filter = if let Some(filter_str) = &opts.hide {
        FmtFilter::from(filter_str.as_str())
    } else {
        FmtFilter::default()
    };

    let fields_fmt = PkgerFieldsFmt::from(&fmt_filter);
    let events_fmt = PkgerEventFmt::from(&fmt_filter);

    tracing_subscriber::fmt::fmt()
        .with_max_level(Level::TRACE)
        .with_env_filter(&filter)
        .fmt_fields(fields_fmt)
        .event_format(events_fmt)
        .init();

    trace!(log_filter = %filter);
    trace!(fmt_filter = ?fmt_filter);
}

/// Fields visitor factory
struct PkgerFields;

impl<'writer> MakeVisitor<&'writer mut dyn fmt::Write> for PkgerFields {
    type Visitor = PkgerFieldsVisitor<'writer>;

    fn make_visitor(&self, target: &'writer mut dyn fmt::Write) -> Self::Visitor {
        PkgerFieldsVisitor::new(target)
    }
}

/// Fields visitor
struct PkgerFieldsVisitor<'writer> {
    writer: &'writer mut dyn fmt::Write,
    err: Option<fmt::Error>,
}
impl<'writer> PkgerFieldsVisitor<'writer> {
    pub fn new(writer: &'writer mut dyn fmt::Write) -> Self {
        Self { writer, err: None }
    }
}

impl<'writer> Visit for PkgerFieldsVisitor<'writer> {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            let dbg = format!(" {:?}", value);
            if let Err(e) = write!(self.writer, "{}", dbg.bold()) {
                self.err = Some(e);
            }
        } else {
            let value = format!("{:#?}", value);
            let field = format!("{}", field);
            if let Err(e) = write!(
                self.writer,
                "{}={}",
                field.truecolor(0xa1, 0xa1, 0xa1),
                value.truecolor(0x26, 0xbd, 0xb0).italic(),
            ) {
                self.err = Some(e);
            }
        }
    }
}

impl<'writer> VisitOutput<fmt::Result> for PkgerFieldsVisitor<'writer> {
    fn finish(self) -> fmt::Result {
        if let Some(e) = self.err {
            Err(e)
        } else {
            Ok(())
        }
    }
}

impl<'writer> VisitFmt for PkgerFieldsVisitor<'writer> {
    fn writer(&mut self) -> &mut dyn fmt::Write {
        self.writer
    }
}

/// Fields formatter
struct PkgerFieldsFmt<'delim> {
    delimiter: &'delim str,
}

impl<'writer, 'delim> FormatFields<'writer> for PkgerFieldsFmt<'delim> {
    fn format_fields<R: RecordFields>(
        &self,
        mut writer: &'writer mut dyn fmt::Write,
        fields: R,
    ) -> fmt::Result {
        let factory = PkgerFields {}.delimited(self.delimiter);
        let mut visitor = factory.make_visitor(&mut writer);
        fields.record(&mut visitor);
        Ok(())
    }
}

impl<'delim> From<&FmtFilter<'delim>> for PkgerFieldsFmt<'delim> {
    fn from(filter: &FmtFilter<'delim>) -> Self {
        PkgerFieldsFmt {
            delimiter: filter.delimiter,
        }
    }
}

struct PkgerEventFmt {
    hide_date: bool,
    hide_fields: bool,
    hide_level: bool,
    hide_spans: bool,
}

impl<'delim> From<&FmtFilter<'delim>> for PkgerEventFmt {
    fn from(filter: &FmtFilter) -> Self {
        Self {
            hide_date: filter.hide_date,
            hide_fields: filter.hide_fields,
            hide_level: filter.hide_level,
            hide_spans: filter.hide_spans,
        }
    }
}

impl<S, N> FormatEvent<S, N> for PkgerEventFmt
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        writer: &mut dyn fmt::Write,
        event: &Event<'_>,
    ) -> fmt::Result {
        if !self.hide_date {
            write!(
                writer,
                "{} ",
                Utc::now().to_rfc3339().bold().truecolor(0x5f, 0x5f, 0x5f)
            )?;
        }
        if !self.hide_level {
            let level = match *event.metadata().level() {
                Level::ERROR => "ERROR".bright_red(),
                Level::WARN => "WARN".bright_yellow(),
                Level::INFO => "INFO".bright_green(),
                Level::DEBUG => "DEBUG".bright_blue(),
                Level::TRACE => "TRACE".bright_magenta(),
            }
            .bold();
            write!(writer, "{} ", level)?;
        }

        ctx.visit_spans::<fmt::Error, _>(|span| {
            if !self.hide_spans {
                write!(writer, "{}", span.name())?;

                let ext = span.extensions();
                let fields = &ext
                    .get::<FormattedFields<N>>()
                    .expect("will never be `None`");

                if !self.hide_fields && !fields.is_empty() {
                    write!(writer, "{}", "{".bold())?;
                    write!(writer, "{}", fields)?;
                    write!(writer, "{}", "}".bold())?;
                }
                write!(writer, "{}", "~>".blue().bold())?;
            }

            Ok(())
        })?;

        ctx.field_format().format_fields(writer, event)?;
        writeln!(writer)
    }
}
