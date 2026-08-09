//! `en16931` — the European e-invoice, on the command line.
//!
//! Everything the libraries do, without writing any Rust: read a UBL, CII or
//! ZUGFeRD document, validate it against the core model or a national CIUS,
//! convert between the two syntaxes, and print the verdict as text, JSON or
//! SVRL.
//!
//! # Exit codes, because this belongs in a pipeline
//!
//! | | |
//! |---|---|
//! | `0` | every document passed |
//! | `1` | a document was read and is **invalid** |
//! | `2` | a document could not be read at all, or the command was misused |
//!
//! Telling `1` from `2` is the whole point: a CI job that treats "this invoice
//! is invalid" and "that path does not exist" the same way will eventually ship
//! an invoice because a mount was missing.

mod input;
mod output;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use en16931::validation::profile::Profile;
use en16931::{Severity, profiles};

/// Exit code for "read it, and it is not valid".
const INVALID: u8 = 1;
/// Exit code for "could not read it, or the command was wrong".
const ERROR: u8 = 2;

#[derive(Parser)]
#[command(
    name = "en16931",
    version,
    about = "Validate, convert, compare and inspect European e-invoices (EN 16931)",
    long_about = None,
    max_term_width = 100,
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Validate one or more documents.
    ///
    /// Reads UBL, CII and ZUGFeRD/Factur-X PDFs, picking the rule set from the
    /// document's own BT-24 unless told otherwise.
    Validate {
        /// Documents to validate. `-` reads standard input.
        #[arg(required = true, value_name = "PATH")]
        paths: Vec<PathBuf>,

        /// Which rule set to apply.
        ///
        /// The default reads BT-24 and uses the profile the document declares,
        /// which is what a receiving system does — validating an XRechnung
        /// against the bare core model is the most common way to ship a
        /// document a counterparty then rejects.
        #[arg(long, value_name = "PROFILE", default_value = "auto")]
        profile: String,

        /// Output shape.
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,

        /// Treat warnings and information as failures.
        ///
        /// Off by default because the authorities do not: KoSIT reports
        /// `BR-CL-23` at warning on purpose, and a build that fails on it fails
        /// on invoices Germany accepts.
        #[arg(long)]
        strict: bool,

        /// Skip a rule, by any of its spellings. Repeatable.
        ///
        /// Recorded on the report and printed, and it makes the run unable to
        /// produce a proof — a deviation you cannot see is worse than one you
        /// argued for.
        #[arg(long = "without", value_name = "RULE")]
        without: Vec<String>,

        /// Print nothing; use the exit code.
        #[arg(long, short)]
        quiet: bool,
    },

    /// Convert a document to the other syntax.
    ///
    /// The conversion goes through the semantic model, so the output is what
    /// EN 16931 says the document means — not a transliteration of its
    /// elements.
    Convert {
        /// The document to convert. `-` reads standard input.
        #[arg(value_name = "PATH")]
        path: PathBuf,

        /// Target syntax.
        #[arg(long, value_enum, value_name = "SYNTAX")]
        to: TargetSyntax,

        /// Validate against this profile first, and stamp BT-24 from it.
        ///
        /// Without it the document is written as it stands, which is right when
        /// you are converting something you did not author. With it, nothing is
        /// written unless the model passes — see `--profile` on `validate`.
        #[arg(long, value_name = "PROFILE")]
        profile: Option<String>,

        /// Write here instead of standard output.
        #[arg(long, short, value_name = "PATH")]
        output: Option<PathBuf>,
    },

    /// Extract the XML payload from a ZUGFeRD / Factur-X PDF.
    ///
    /// The bytes come out **verbatim**: whoever diagnoses a rejected invoice
    /// needs what the counterparty sent, not a reconstruction of it.
    Extract {
        /// The hybrid PDF. `-` reads standard input.
        #[arg(value_name = "PATH")]
        path: PathBuf,

        /// Write here instead of standard output.
        #[arg(long, short, value_name = "PATH")]
        output: Option<PathBuf>,
    },

    /// Compare two documents as invoices, not as XML.
    ///
    /// Both are read into the semantic model first, so a UBL invoice and its
    /// CII translation compare **equal** where an XML diff would show two
    /// unrelated files. That is the question worth asking of a conversion, a
    /// migration, or a counterparty who says they received something else.
    Diff {
        /// The document to compare from. `-` reads standard input.
        #[arg(value_name = "LEFT")]
        left: PathBuf,

        /// The document to compare to.
        #[arg(value_name = "RIGHT")]
        right: PathBuf,

        /// Output shape. SVRL describes a verdict, not a comparison, so it is
        /// refused.
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },

    /// Say what a document is, without validating it.
    ///
    /// Syntax, declared profile, the totals, and anything the reader could not
    /// map. The first command to run on a document you were sent.
    Inspect {
        /// Documents to inspect. `-` reads standard input.
        #[arg(required = true, value_name = "PATH")]
        paths: Vec<PathBuf>,

        /// Output shape. SVRL is not a document description, so it is refused.
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },

    /// Look a business rule up by id — `BR-CO-14`, `br-co-14`, `BR-CO-3`.
    Explain {
        /// The rule id, in any of its spellings.
        #[arg(value_name = "RULE")]
        rule: String,
    },

    /// List the profiles this build can validate against.
    Profiles,

    /// Print the whole rule catalogue — every id, severity, provenance and text.
    ///
    /// The thing to diff across releases, feed to a documentation build, or grep
    /// when a counterparty quotes an id you do not recognise.
    Rules {
        /// Only rules a profile runs, by name or BT-24. Default: everything.
        #[arg(long, value_name = "PROFILE")]
        profile: Option<String>,

        /// Only rules touching this business term — `BT-117`, or `117`.
        #[arg(long, value_name = "TERM")]
        term: Option<String>,

        /// Output shape.
        #[arg(long, value_enum, default_value_t = CatalogueFormat::Text)]
        format: CatalogueFormat,
    },

    /// Print a shell completion script, or the man page.
    ///
    /// ```sh
    /// en16931 generate bash > /usr/share/bash-completion/completions/en16931
    /// en16931 generate man  > /usr/share/man/man1/en16931.1
    /// ```
    Generate {
        /// What to write to standard output.
        #[arg(value_name = "WHAT")]
        what: Generate,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum CatalogueFormat {
    /// One line per rule, aligned.
    Text,
    /// A JSON array, for a documentation build or a diff.
    Json,
}

#[derive(Clone, Copy, ValueEnum)]
enum Generate {
    Bash,
    Zsh,
    Fish,
    #[value(name = "powershell")]
    PowerShell,
    Elvish,
    /// A `man(1)` page for the top-level command.
    Man,
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    /// For a person.
    Text,
    /// The stable interchange shape — see `en16931::report`.
    Json,
    /// What every Schematron tool in this field speaks.
    Svrl,
}

#[derive(Clone, Copy, ValueEnum)]
enum TargetSyntax {
    /// OASIS UBL 2.1.
    Ubl,
    /// UN/CEFACT Cross Industry Invoice D16B.
    Cii,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("en16931: {message}");
            ExitCode::from(ERROR)
        }
    }
}

/// Whether an I/O failure is just the reader having gone away.
///
/// `en16931 convert x.xml --to cii | head` closes the pipe, and Rust's default
/// is to report that as an error on stderr. It is not one: the user asked for
/// the first few lines and got them. Every well-behaved command-line tool is
/// silent here.
fn is_broken_pipe(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::BrokenPipe
}

fn run(cli: Cli) -> Result<ExitCode, String> {
    match cli.command {
        Command::Validate {
            paths,
            profile,
            format,
            strict,
            without,
            quiet,
        } => validate(&paths, &profile, format, strict, &without, quiet),
        Command::Convert {
            path,
            to,
            profile,
            output,
        } => convert(&path, to, profile.as_deref(), output.as_deref()),
        Command::Extract { path, output } => extract(&path, output.as_deref()),
        Command::Inspect { paths, format } => inspect(&paths, format),
        Command::Diff {
            left,
            right,
            format,
        } => diff(&left, &right, format),
        Command::Explain { rule } => explain(&rule),
        Command::Profiles => {
            let mut out = String::new();
            output::profiles(&mut out);
            write_out(None, out.as_bytes())?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Rules {
            profile,
            term,
            format,
        } => rules(profile.as_deref(), term.as_deref(), format),
        Command::Generate { what } => generate(what),
    }
}

// ── rules ────────────────────────────────────────────────────────────────────

fn rules(
    profile: Option<&str>,
    term: Option<&str>,
    format: CatalogueFormat,
) -> Result<ExitCode, String> {
    let profile = match profile {
        Some(name) => Some(resolve(name)?.ok_or("--profile auto is not meaningful here")?),
        None => None,
    };
    let term = match term {
        // `BT-117` and `117` are the same question asked by two kinds of user.
        Some(t) => Some(
            t.trim_start_matches("BT-")
                .trim_start_matches("bt-")
                .parse::<u16>()
                .map(en16931::BtId)
                .map_err(|_| format!("not a business term: {t:?}. Try `BT-117` or `117`."))?,
        ),
        None => None,
    };
    let mut out = String::new();
    output::catalogue(&mut out, profile, term, format);
    write_out(None, out.as_bytes())?;
    Ok(ExitCode::SUCCESS)
}

// ── generate ─────────────────────────────────────────────────────────────────

fn generate(what: Generate) -> Result<ExitCode, String> {
    use clap::CommandFactory as _;
    let mut cmd = Cli::command();
    let mut out = Vec::new();
    match what {
        Generate::Man => clap_mangen::Man::new(cmd)
            .render(&mut out)
            .map_err(|e| format!("man: {e}"))?,
        shell => {
            let shell = match shell {
                Generate::Bash => clap_complete::Shell::Bash,
                Generate::Zsh => clap_complete::Shell::Zsh,
                Generate::Fish => clap_complete::Shell::Fish,
                Generate::PowerShell => clap_complete::Shell::PowerShell,
                Generate::Elvish => clap_complete::Shell::Elvish,
                Generate::Man => unreachable!("handled above"),
            };
            clap_complete::generate(shell, &mut cmd, "en16931", &mut out);
        }
    }
    write_out(None, &out)?;
    Ok(ExitCode::SUCCESS)
}

// ── validate ─────────────────────────────────────────────────────────────────

fn validate(
    paths: &[PathBuf],
    profile: &str,
    format: Format,
    strict: bool,
    without: &[String],
    quiet: bool,
) -> Result<ExitCode, String> {
    let selected = resolve(profile)?;
    let mut worst = ExitCode::SUCCESS;
    let mut reports = Vec::new();

    for path in paths {
        let loaded = input::load(path).map_err(|e| e.to_string())?;
        // `auto` asks the document. §7.6 exists for exactly this: BT-24 is there
        // so a receiver can apply the rules the sender generated under.
        let profile = selected.unwrap_or_else(|| declared(&loaded.invoice));
        let report = if without.is_empty() {
            profile.validate(&loaded.invoice)
        } else {
            let mut check = en16931::validation::Check::new(profile);
            for rule in without {
                check = check.without(rule.clone());
            }
            check.run(&loaded.invoice)
        };
        let failed = !report.is_valid()
            || (strict
                && report
                    .findings()
                    .iter()
                    .any(|f| f.severity != Severity::Fatal));
        if failed {
            worst = ExitCode::from(INVALID);
        }
        reports.push((loaded, report));
    }

    if !quiet {
        let mut out = String::new();
        output::validation(&mut out, &reports, format);
        write_out(None, out.as_bytes())?;
    }
    Ok(worst)
}

/// The profile a document declares, falling back to the core model.
///
/// Never a guess: an unknown BT-24 means the sender used a usage specification
/// this build does not carry, and checking it against the core rules is the
/// most that can honestly be said about it.
fn declared(invoice: &en16931::Invoice) -> &'static Profile {
    invoice
        .specification_id
        .as_deref()
        .and_then(profiles::for_specification_id)
        .unwrap_or(&profiles::EN16931)
}

/// Resolve `--profile`. `auto` yields `None`, meaning "ask each document".
fn resolve(name: &str) -> Result<Option<&'static Profile>, String> {
    if name.eq_ignore_ascii_case("auto") {
        return Ok(None);
    }
    // By short name, then by the BT-24 identifier — a script has the second and
    // a person has the first, and both are unambiguous.
    profiles::ALL
        .iter()
        .copied()
        .find(|p| p.id.eq_ignore_ascii_case(name) || p.specification_id == name)
        .map(Some)
        .ok_or_else(|| {
            let known: Vec<&str> = profiles::ALL.iter().map(|p| p.id).collect();
            format!(
                "unknown profile {name:?}. Known: auto, {}",
                known.join(", ")
            )
        })
}

// ── convert ──────────────────────────────────────────────────────────────────

fn convert(
    path: &std::path::Path,
    to: TargetSyntax,
    profile: Option<&str>,
    out: Option<&std::path::Path>,
) -> Result<ExitCode, String> {
    let loaded = input::load(path).map_err(|e| e.to_string())?;
    let profile = match profile {
        Some(name) => Some(resolve(name)?.ok_or("--profile auto is not meaningful here")?),
        None => None,
    };

    // `ubl::Written` and `cii::Written` are distinct types with the same two
    // fields, so each arm reduces to that pair rather than to a common type
    // neither crate declares.
    let written: Result<(String, Vec<String>), String> = match (to, profile) {
        (TargetSyntax::Ubl, None) => {
            let w = en16931_formats::ubl::write(&loaded.invoice);
            Ok((w.xml, w.dropped))
        }
        (TargetSyntax::Cii, None) => {
            let w = en16931_formats::cii::write(&loaded.invoice);
            Ok((w.xml, w.dropped))
        }
        (TargetSyntax::Ubl, Some(p)) => en16931_formats::ubl::write_for(&loaded.invoice, p)
            .map(|w| (w.xml, w.dropped))
            .map_err(|e| format!("{e}\n{}", e.report())),
        (TargetSyntax::Cii, Some(p)) => en16931_formats::cii::write_for(&loaded.invoice, p)
            .map(|w| (w.xml, w.dropped))
            .map_err(|e| format!("{e}\n{}", e.report())),
    };
    let (xml, dropped) = match written {
        Ok(pair) => pair,
        Err(message) => {
            eprintln!("en16931: {message}");
            return Ok(ExitCode::from(INVALID));
        }
    };

    // Anything the target syntax could not carry goes to stderr, so a redirected
    // stdout is the document and nothing else — and the loss is still visible.
    for d in &dropped {
        eprintln!("en16931: dropped, unrepresentable in the target syntax: {d}");
    }
    for n in &loaded.notes {
        eprintln!("en16931: {n}");
    }
    write_out(out, xml.as_bytes())?;
    Ok(ExitCode::SUCCESS)
}

// ── extract ──────────────────────────────────────────────────────────────────

fn extract(path: &std::path::Path, out: Option<&std::path::Path>) -> Result<ExitCode, String> {
    let bytes = if path == std::path::Path::new("-") {
        use std::io::Read as _;
        let mut buf = Vec::new();
        std::io::stdin()
            .read_to_end(&mut buf)
            .map_err(|e| format!("-: {e}"))?;
        buf
    } else {
        std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?
    };
    let got = en16931_formats::zugferd::extract(&bytes)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    for d in &got.divergence {
        eprintln!("en16931: {d}");
    }
    write_out(out, got.xml.as_bytes())?;
    Ok(ExitCode::SUCCESS)
}

// ── inspect ──────────────────────────────────────────────────────────────────

fn inspect(paths: &[PathBuf], format: Format) -> Result<ExitCode, String> {
    let mut loaded = Vec::new();
    for path in paths {
        loaded.push(input::load(path).map_err(|e| e.to_string())?);
    }
    let mut out = String::new();
    match format {
        Format::Text => output::inspect_text(&mut out, &loaded),
        Format::Json => output::inspect_json(&mut out, &loaded),
        Format::Svrl => {
            return Err(
                "SVRL is a validation report format; `inspect` does not validate. \
                 Use --format json, or `validate --format svrl`."
                    .to_owned(),
            );
        }
    }
    write_out(None, out.as_bytes())?;
    Ok(ExitCode::SUCCESS)
}

// ── diff ─────────────────────────────────────────────────────────────────────

/// Compare two documents as invoices.
///
/// # Why this is a comparison nothing XML-first can make
///
/// `diff a.xml b.xml` on two e-invoices answers a question nobody asked. The
/// same invoice written as UBL and as CII shares almost no text; the same
/// invoice written twice by the same system differs in whitespace, element
/// order and optional wrappers. What a person actually wants to know is
/// *"do these say the same thing?"*, and that is a question about the model.
///
/// So both sides are read into an [`Invoice`](en16931::Invoice) first. Two
/// documents in **different syntaxes** compare equal when they carry the same
/// invoice — which is the whole point of a conversion, and the thing you would
/// otherwise verify by reading.
///
/// # Exit codes
///
/// `0` identical, `1` they differ, `2` one could not be read. Same shape as
/// `validate`, and for the same reason: a pipeline has to tell "the answer is
/// no" from "I could not answer".
fn diff(left: &Path, right: &Path, format: Format) -> Result<ExitCode, String> {
    let a = input::load(left).map_err(|e| e.to_string())?;
    let b = input::load(right).map_err(|e| e.to_string())?;

    let differences = output::model_differences(&a.invoice, &b.invoice)?;
    let mut out = String::new();
    match format {
        Format::Text => output::diff_text(&mut out, &a, &b, &differences),
        Format::Json => output::diff_json(&mut out, &a, &b, &differences)?,
        Format::Svrl => {
            return Err(
                "SVRL describes a verdict, not a comparison. Use --format json.".to_owned(),
            );
        }
    }
    write_out(None, out.as_bytes())?;
    Ok(if differences.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(INVALID)
    })
}

// ── explain ──────────────────────────────────────────────────────────────────

fn explain(query: &str) -> Result<ExitCode, String> {
    let mut out = String::new();
    if let Some(rule) = en16931::validation::rules::explain(query) {
        output::rule(&mut out, rule);
        write_out(None, out.as_bytes())?;
        return Ok(ExitCode::SUCCESS);
    }
    // Restrictions are data rather than predicates, so they are not rules — and
    // they appear in reports under their real ids, so a user holding one
    // deserves an answer rather than "no such rule".
    if let Some((profile, restriction)) = en16931::validation::rules::explain_restriction(query) {
        output::restriction(&mut out, profile, restriction);
        write_out(None, out.as_bytes())?;
        return Ok(ExitCode::SUCCESS);
    }
    Err(format!(
        "no rule or restriction called {query:?}. Ids are matched loosely — \
         `BR-CO-3`, `BR-CO-03` and `br-co-3` are the same rule, and the \
         standard's `BR-IG-*` / `BR-IP-*` reach the artefacts' `BR-AF-*` / \
         `BR-AG-*`."
    ))
}

// ── shared ───────────────────────────────────────────────────────────────────

/// The one place anything reaches standard output.
///
/// Every command builds its output into a `String` and hands it here, rather
/// than calling `println!`. That is not tidiness: `println!` **panics** when the
/// reader has gone away, so `en16931 rules | head` printed a Rust backtrace
/// where every other command-line tool prints nothing. Funnelling the writes
/// makes the one place that has to know about `BrokenPipe` the one place that
/// does, and makes each command a single `write(2)`.
fn write_out(path: Option<&std::path::Path>, bytes: &[u8]) -> Result<(), String> {
    match path {
        Some(p) => std::fs::write(p, bytes).map_err(|e| format!("{}: {e}", p.display())),
        None => {
            use std::io::Write as _;
            match std::io::stdout().write_all(bytes) {
                Ok(()) => Ok(()),
                Err(e) if is_broken_pipe(&e) => Ok(()),
                Err(e) => Err(format!("stdout: {e}")),
            }
        }
    }
}
