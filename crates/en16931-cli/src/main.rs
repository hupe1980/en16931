//! `en16931`: validate, convert, compare, inspect and embed EN 16931
//! e-invoices (UBL, CII, ZUGFeRD / Factur-X) on the command line.
//!
//! # Exit codes
//!
//! `validate` exits with the recommendation `en16931_formats::receive` gives:
//!
//! | | |
//! |---|---|
//! | `0` | every document is **accepted** |
//! | `1` | a document is **rejected** — invalid, unreadable bytes, or a BT-24 no profile matches — or **undefined** (ZUGFeRD MINIMUM / BASIC WL: not an EN 16931 invoice) |
//! | `2` | a file could not be opened, or the command was misused |
//!
//! Bytes that are not an invoice are a rejection (`1`); a missing file is `2`.

#![forbid(unsafe_code)]

mod input;
mod output;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use en16931::validation::profile::Profile;
use en16931::{Severity, profiles};
use en16931_formats::{Loss, Policy, Recommendation, Syntax, zugferd};

/// Exit code for "rejected", "undefined" or "differs".
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
    /// Receive one or more documents: validate and decide.
    ///
    /// Reads UBL, CII and ZUGFeRD/Factur-X PDFs, checks each under the profile
    /// its own BT-24 declares and, with `--require`, under the profile you
    /// require, and recommends accept, reject or undefined.
    Validate {
        /// Documents to validate. `-` reads standard input.
        #[arg(required = true, value_name = "PATH")]
        paths: Vec<PathBuf>,

        /// The profile you require, by name from `en16931 profiles` or BT-24.
        ///
        /// Adds a second check beside the declared profile (BT-24); a German
        /// public buyer requires `xrechnung`.
        #[arg(long, short, value_name = "PROFILE")]
        require: Option<String>,

        /// Report a rule at another severity: `RULE=fatal|warning|information`.
        /// Repeatable.
        ///
        /// A receiver's deviation, like KoSIT's `customLevel`. Recorded on the
        /// report; the run cannot produce a proof.
        #[arg(long, value_name = "RULE=LEVEL")]
        relevel: Vec<String>,

        /// Skip a rule, by any of its spellings. Repeatable.
        ///
        /// Recorded on the report as not checked; the run cannot produce a proof.
        #[arg(long = "without", value_name = "RULE")]
        without: Vec<String>,

        /// Output shape.
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,

        /// Fail an accepted document that reports anything at all — a warning,
        /// an unmapped element.
        ///
        /// Off by default: authorities accept documents with warnings.
        #[arg(long)]
        strict: bool,

        #[command(flatten)]
        limits: LimitArgs,

        /// Print nothing; use the exit code.
        #[arg(long, short)]
        quiet: bool,
    },

    /// Convert a document to the other syntax.
    ///
    /// Goes through the semantic model. Anything that would not reach the
    /// target refuses the conversion unless `--allow-loss` is given; what
    /// was derived or lost is printed to standard error.
    Convert {
        /// The document to convert. `-` reads standard input.
        #[arg(value_name = "PATH")]
        path: PathBuf,

        /// Target syntax.
        #[arg(long, value_enum, value_name = "SYNTAX")]
        to: TargetSyntax,

        /// Convert even when something would be lost, and list it.
        #[arg(long)]
        allow_loss: bool,

        /// Convert only a document `validate --require PROFILE` accepts.
        #[arg(long, short, value_name = "PROFILE")]
        require: Option<String>,

        /// Write here instead of standard output.
        #[arg(long, short, value_name = "PATH")]
        output: Option<PathBuf>,
    },

    /// Extract the XML payload from a ZUGFeRD / Factur-X PDF.
    ///
    /// The bytes come out verbatim.
    Extract {
        /// The hybrid PDF. `-` reads standard input.
        #[arg(value_name = "PATH")]
        path: PathBuf,

        /// Write here instead of standard output.
        #[arg(long, short, value_name = "PATH")]
        output: Option<PathBuf>,

        #[command(flatten)]
        limits: LimitArgs,
    },

    /// Embed an invoice into a PDF/A-3 file: a ZUGFeRD / Factur-X hybrid.
    ///
    /// The invoice — UBL, CII or the JSON envelope — is validated under the
    /// profile its BT-24 declares (or `--profile`), which must be `en16931` or
    /// `xrechnung`, and written as CII under `factur-x.xml` or `xrechnung.xml`.
    /// The PDF must already be PDF/A-3: nothing is converted. The result is the
    /// PDF followed by one incremental update, read back before it is written.
    ///
    ///     en16931 embed rendered.pdf invoice.xml -o invoice.pdf
    #[command(verbatim_doc_comment)]
    Embed {
        /// The PDF/A-3 rendering of the invoice.
        #[arg(value_name = "PDF")]
        pdf: PathBuf,

        /// The invoice: UBL, CII or the JSON envelope. `-` reads standard input.
        #[arg(value_name = "INVOICE")]
        invoice: PathBuf,

        /// Where to write the hybrid PDF.
        #[arg(long, short, value_name = "PATH")]
        output: PathBuf,

        /// Validate and embed under this profile instead of the one BT-24
        /// declares: `en16931` or `xrechnung`.
        #[arg(long, short, value_name = "PROFILE")]
        profile: Option<String>,

        /// `/AFRelationship`: `alternative` (German guidance; the default) or
        /// `source` (PDFlib's for Factur-X sent abroad).
        #[arg(long, value_enum, default_value_t = RelationshipArg::Alternative)]
        relationship: RelationshipArg,
    },

    /// Compare two documents as invoices, not as XML.
    ///
    /// Both are read into the semantic model first, so a UBL invoice and its
    /// CII translation compare equal.
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
    /// Syntax, declared profile, the totals, and everything the reader had to
    /// say.
    Inspect {
        /// Documents to inspect. `-` reads standard input.
        #[arg(required = true, value_name = "PATH")]
        paths: Vec<PathBuf>,

        /// Output shape. JSON is the report shape with no rules run; SVRL is a
        /// verdict, so it is refused.
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

    /// May these VAT categories share one invoice? Answered before you build it
    ///
    /// Needs no document: the exclusivity rules depend on the codes alone.
    ///
    ///     en16931 categories S Z      # ordinary: standard beside zero-rated
    ///     en16931 categories S O      # refused — BR-O-11…14
    ///
    /// Exit 0 if they can share a document, 1 if they cannot, 2 if a code is
    /// not a VAT category — the same three answers as validate.
    #[command(verbatim_doc_comment)]
    Categories {
        /// UNCL 5305 codes: `S`, `Z`, `E`, `AE`, `K`, `G`, `O`, `L`, `M`, `B`.
        #[arg(required = true, value_name = "CODE")]
        codes: Vec<String>,
    },

    /// Print the whole rule catalogue — every id, severity, provenance and text.
    ///
    /// Derived from the registry; diff it across releases.
    Rules {
        /// Only rules a profile runs, by name or BT-24. Default: everything.
        #[arg(long, short, value_name = "PROFILE")]
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
    ///     en16931 generate bash > /usr/share/bash-completion/completions/en16931
    ///     en16931 generate man  > /usr/share/man/man1/en16931.1
    //
    // `verbatim_doc_comment`: clap would reflow the shell commands above.
    #[command(verbatim_doc_comment)]
    Generate {
        /// What to write to standard output.
        #[arg(value_name = "WHAT")]
        what: Generate,
    },
}

/// The bounds a hybrid PDF is extracted under. The defaults are the
/// library's; lower them for a service that receives from strangers.
#[derive(Args)]
struct LimitArgs {
    /// The most bytes any one PDF stream may inflate to.
    #[arg(long, value_name = "BYTES", default_value_t = zugferd::Limits::DEFAULT_MAX_STREAM_BYTES)]
    max_stream_bytes: usize,

    /// The most bytes one PDF may inflate in all.
    #[arg(long, value_name = "BYTES", default_value_t = zugferd::Limits::DEFAULT_MAX_TOTAL_BYTES)]
    max_total_bytes: usize,

    /// The most PDF objects one walk may visit.
    #[arg(long, value_name = "COUNT", default_value_t = zugferd::Limits::DEFAULT_MAX_OBJECTS)]
    max_objects: usize,
}

impl LimitArgs {
    fn limits(&self) -> zugferd::Limits {
        zugferd::Limits::new()
            .with_max_stream_bytes(self.max_stream_bytes)
            .with_max_total_bytes(self.max_total_bytes)
            .with_max_objects(self.max_objects)
    }
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
    /// The interchange shape, `en16931-report/4` — see `en16931::report`.
    Json,
    /// What every Schematron tool in this field speaks.
    Svrl,
}

#[derive(Clone, Copy, ValueEnum)]
enum RelationshipArg {
    /// The XML is another representation of the same invoice.
    Alternative,
    /// The pages were produced from the XML.
    Source,
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

/// Whether an I/O failure is the reader having gone away (`… | head`), which
/// is not reported.
fn is_broken_pipe(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::BrokenPipe
}

fn run(cli: Cli) -> Result<ExitCode, String> {
    match cli.command {
        Command::Validate {
            paths,
            require,
            relevel,
            without,
            format,
            strict,
            limits,
            quiet,
        } => {
            let policy = Policy {
                require: require.as_deref().map(resolve).transpose()?,
                relevel: relevel
                    .iter()
                    .map(|r| relevelling(r))
                    .collect::<Result<_, _>>()?,
                without,
                limits: limits.limits(),
            };
            validate(&paths, &policy, format, strict, quiet)
        }
        Command::Convert {
            path,
            to,
            allow_loss,
            require,
            output,
        } => convert(&path, to, allow_loss, require.as_deref(), output.as_deref()),
        Command::Extract {
            path,
            output,
            limits,
        } => extract(&path, output.as_deref(), &limits.limits()),
        Command::Embed {
            pdf,
            invoice,
            output,
            profile,
            relationship,
        } => embed(&pdf, &invoice, &output, profile.as_deref(), relationship),
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
        Command::Categories { codes } => categories(&codes),
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
        Some(name) => Some(resolve(name)?),
        None => None,
    };
    let term = match term {
        // `BT-117` and `117` are the same term.
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
    policy: &Policy,
    format: Format,
    strict: bool,
    quiet: bool,
) -> Result<ExitCode, String> {
    let mut worst = ExitCode::SUCCESS;
    let mut results = Vec::new();

    for path in paths {
        let bytes = input::bytes(path)?;
        let received = en16931_formats::receive(&bytes, policy);
        let accepted = received.recommendation == Recommendation::Accept
            && !(strict && reports_anything(&received));
        if !accepted {
            worst = ExitCode::from(INVALID);
        }
        results.push((path.clone(), received));
    }

    if !quiet {
        let mut out = String::new();
        output::validation(&mut out, &results, format);
        write_out(None, out.as_bytes())?;
    }
    Ok(worst)
}

/// Whether either report holds anything at all — what `--strict` fails on.
fn reports_anything(r: &en16931_formats::Received) -> bool {
    std::iter::once(&r.report_declared)
        .chain(&r.report_required)
        .any(|rep| rep.most_severe().is_some())
}

/// Resolve a profile by slug, display name, alias or BT-24, via the library's
/// parser.
fn resolve(name: &str) -> Result<&'static Profile, String> {
    name.parse()
        .map_err(|e: profiles::UnknownProfile| e.to_string())
}

/// `BR-CL-23=warning` → the rule and its new severity.
fn relevelling(arg: &str) -> Result<(String, Severity), String> {
    let (rule, level) = arg
        .split_once('=')
        .ok_or_else(|| format!("--relevel {arg:?}: expected RULE=LEVEL, as BR-CL-23=warning"))?;
    let level = level
        .parse::<Severity>()
        .map_err(|e| format!("--relevel {arg:?}: {e}"))?;
    Ok((rule.trim().to_owned(), level))
}

// ── convert ──────────────────────────────────────────────────────────────────

fn convert(
    path: &Path,
    to: TargetSyntax,
    allow_loss: bool,
    require: Option<&str>,
    out: Option<&Path>,
) -> Result<ExitCode, String> {
    let bytes = input::bytes(path)?;
    if let Some(name) = require {
        let policy = Policy {
            require: Some(resolve(name)?),
            ..Policy::default()
        };
        let received = en16931_formats::receive(&bytes, &policy);
        if received.recommendation != Recommendation::Accept {
            eprintln!("en16931: not converted: {}\n{received}", path.display());
            return Ok(ExitCode::from(INVALID));
        }
    }

    let syntax = match to {
        TargetSyntax::Ubl => Syntax::Ubl,
        TargetSyntax::Cii => Syntax::Cii,
    };
    let loss = if allow_loss {
        Loss::Allow
    } else {
        Loss::Refuse
    };
    let converted = match en16931_formats::convert(&bytes, syntax, loss) {
        Ok(c) => c,
        Err(en16931_formats::ConvertError::Read(e)) => {
            return Err(format!("{}: {e}", path.display()));
        }
        Err(en16931_formats::ConvertError::Lossy { lost }) => {
            eprintln!(
                "en16931: {}: not converted — {} item(s) would be lost; \
                 --allow-loss converts and lists them",
                path.display(),
                lost.len()
            );
            for e in &lost {
                eprintln!("  {e}");
            }
            return Ok(ExitCode::from(INVALID));
        }
        Err(en16931_formats::ConvertError::Unwritable(u)) => {
            eprintln!("en16931: {}: {u}", path.display());
            for e in &u.entries {
                eprintln!("  {e}");
            }
            return Ok(ExitCode::from(INVALID));
        }
    };

    // Reader and writer entries go to stderr; stdout is only the document.
    for e in converted.report.entries().chain(&converted.written) {
        eprintln!("en16931: {e}");
    }
    if !converted.report.is_valid() {
        eprintln!(
            "en16931: the source is not valid as it declares itself; \
             `en16931 validate` says why"
        );
    }
    write_out(out, converted.document.as_bytes())?;
    Ok(ExitCode::SUCCESS)
}

// ── extract ──────────────────────────────────────────────────────────────────

fn extract(path: &Path, out: Option<&Path>, limits: &zugferd::Limits) -> Result<ExitCode, String> {
    let bytes = input::bytes(path)?;
    let got = zugferd::extract(&bytes, limits).map_err(|e| format!("{}: {e}", path.display()))?;
    for d in &got.divergence {
        eprintln!("en16931: {d}");
    }
    write_out(out, &got.xml)?;
    Ok(ExitCode::SUCCESS)
}

// ── embed ────────────────────────────────────────────────────────────────────

fn embed(
    pdf: &Path,
    invoice: &Path,
    out: &Path,
    profile: Option<&str>,
    relationship: RelationshipArg,
) -> Result<ExitCode, String> {
    use en16931::profiles::{En16931, XRechnung};
    use en16931::validation::profile::Validated;

    let pdf_bytes = input::bytes(pdf)?;
    let bytes = input::bytes(invoice)?;
    let model = if bytes.trim_ascii_start().starts_with(b"{") {
        serde_json::from_slice::<en16931::Invoice>(&bytes)
            .map_err(|e| format!("{}: {e}", invoice.display()))?
    } else {
        en16931_formats::read(&bytes)
            .map_err(|e| format!("{}: {e}", invoice.display()))?
            .invoice
    };
    let target = match profile {
        Some(name) => resolve(name)?,
        None => match profiles::classify(model.specification_id.as_deref()).profile() {
            Some(p) => p,
            None => {
                eprintln!(
                    "en16931: {}: BT-24 names no profile this build knows; pass --profile en16931 or xrechnung",
                    invoice.display()
                );
                return Ok(ExitCode::from(INVALID));
            }
        },
    };
    let options = zugferd::EmbedOptions {
        relationship: match relationship {
            RelationshipArg::Alternative => zugferd::Relationship::Alternative,
            RelationshipArg::Source => zugferd::Relationship::Source,
        },
        ..zugferd::EmbedOptions::default()
    };
    let result = if std::ptr::eq(target, &profiles::EN16931) {
        match Validated::<En16931>::new(model) {
            Ok(proof) => zugferd::embed(&pdf_bytes, &proof, &options),
            Err(rejected) => return Ok(not_valid(invoice, rejected.report())),
        }
    } else if std::ptr::eq(target, &profiles::XRECHNUNG) {
        match Validated::<XRechnung>::new(model) {
            Ok(proof) => zugferd::embed(&pdf_bytes, &proof, &options),
            Err(rejected) => return Ok(not_valid(invoice, rejected.report())),
        }
    } else {
        eprintln!(
            "en16931: {}: {} has no ZUGFeRD / Factur-X carrier; embed writes EN 16931 and XRechnung",
            invoice.display(),
            target.id
        );
        return Ok(ExitCode::from(INVALID));
    };
    match result {
        Ok(hybrid) => {
            write_out(Some(out), &hybrid)?;
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => {
            eprintln!("en16931: {}: not embedded: {e}", pdf.display());
            Ok(ExitCode::from(INVALID))
        }
    }
}

/// An invoice that does not pass the profile it is embedded under.
fn not_valid(path: &Path, report: &en16931::ValidationReport) -> ExitCode {
    eprintln!(
        "en16931: {}: not embedded — the invoice is not valid\n{report}",
        path.display()
    );
    ExitCode::from(INVALID)
}

// ── inspect ──────────────────────────────────────────────────────────────────

fn inspect(paths: &[PathBuf], format: Format) -> Result<ExitCode, String> {
    let mut loaded = Vec::new();
    for path in paths {
        loaded.push(input::load(path)?);
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

/// Compare two documents as invoices, through the model, so different
/// syntaxes carrying the same invoice compare equal.
///
/// Exits `0` identical, `1` they differ, `2` one could not be read.
fn diff(left: &Path, right: &Path, format: Format) -> Result<ExitCode, String> {
    let a = input::load(left)?;
    let b = input::load(right)?;

    let differences = output::model_differences(&a.read.invoice, &b.read.invoice)?;
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
    // Profile restrictions (§7.3.2) are not rules but are reported under ids.
    if let Some((profile, restriction)) = en16931::validation::rules::explain_restriction(query) {
        output::restriction(&mut out, profile, restriction);
        write_out(None, out.as_bytes())?;
        return Ok(ExitCode::SUCCESS);
    }
    Err(format!(
        "no rule or restriction called {query:?}. Ids are matched loosely — \
         `BR-CO-3`, `BR-CO-03` and `br-co-3` are the same rule. The IGIC and \
         IPSI rules are `BR-IG-*` and `BR-IP-*` (`BR-AF-*` / `BR-AG-*` before CEN 1.3.17)."
    ))
}

/// `en16931 categories S O`: may these share one invoice? Takes no document.
fn categories(codes: &[String]) -> Result<ExitCode, String> {
    let mut cats = Vec::with_capacity(codes.len());
    for code in codes {
        let cat = en16931::VatCategory::from_code(code).ok_or_else(|| {
            // Built with `push_str`: `rustfmt` would bake indentation into a
            // `\`-continued literal.
            let known: Vec<&str> = en16931::VatCategory::ALL.iter().map(|c| c.code()).collect();
            let mut msg = format!("{code:?} is not a VAT category code. ");
            msg.push_str("EN 16931 has ten, and BR-CL-17 compares them literally, so they are ");
            msg.push_str("case-sensitive: ");
            msg.push_str(&known.join(", "));
            msg
        })?;
        cats.push(cat);
    }

    let mut out = String::new();
    output::categories(&mut out, &cats);
    write_out(None, out.as_bytes())?;
    Ok(match en16931::VatCategory::can_share_document(&cats) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::from(INVALID),
    })
}

// ── shared ───────────────────────────────────────────────────────────────────

/// The one place anything reaches standard output (or `path`).
///
/// Not `println!`, which panics on a closed pipe; a broken pipe is silent here.
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
