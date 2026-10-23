//! User-owned mechanistic gate between ca and every provider.
//!
//! Rules, URL list, adjustments, and the optional trip file are the user's.
//! Matching is literal, regular-expression, or URL comparison in this process.
//! Adjustments rewrite outbound text before a provider sees it, including text
//! inside a file or a page. A literal keyword is changed in any case, matching
//! the same detection used for phrases. Tools still open the real file and
//! fetch the real URL. A page stand-in rewrites words in a fetched page into
//! Latin or a configured alphabet. The screen puts those words back. Request
//! rules then scan that rewritten text — file reads, tool output, command
//! output, and the request body — before omission or truncation. Responses
//! are not rewritten. A trip is sticky for the process: the current session
//! stops, and no later provider bytes are delivered until a new process starts.

use crate::agent::stream::ProviderError;
use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

const MAX_RULES: usize = 32;
const MAX_PATTERN_BYTES: usize = 256;
const MAX_ID_BYTES: usize = 64;
const RESPONSE_CARRY_BYTES: usize = 8 * 1024;
const TRIP_POLL_INTERVAL: Duration = Duration::from_millis(200);
const MAX_TRIP_FILE_BYTES: u64 = 64;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProviderFilterTarget {
    Request,
    Response,
    #[default]
    Both,
}

impl ProviderFilterTarget {
    fn covers(self, direction: Direction) -> bool {
        matches!(
            (self, direction),
            (Self::Both, _)
                | (Self::Request, Direction::Request)
                | (Self::Response, Direction::Response)
        )
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProviderFilterKind {
    #[default]
    Literal,
    Regex,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProviderFilterRule {
    pub id: String,
    /// `request` watches bytes ca would send. `response` watches provider
    /// replies. `both` watches either direction and is the default.
    #[serde(default)]
    pub target: ProviderFilterTarget,
    /// `literal` is a word or phrase in any case. `regex` is a regular expression.
    #[serde(default)]
    pub kind: ProviderFilterKind,
    pub pattern: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProviderFilterConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trip_file: Option<PathBuf>,
    /// Quoted words and phrases, matched in any case in either direction.
    /// `"A-123"` matches `A-123`, `a-123`, and the same text inside quotation
    /// marks. A trip names `phrase-1`, `phrase-2`, and so on, never the phrase.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub phrases: Vec<String>,
    /// Absolute `http` or `https` URLs. `http://stop.com` matches that URL in
    /// any host case, with or without a trailing slash. `http://stop.com/stop`
    /// matches that path. Neither matches `http://anything.else`, a lookalike
    /// host, or a different path. A trip names `url-1`, `url-2`, and so on,
    /// never the URL.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub urls: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<ProviderFilterRule>,
    /// Rewrite outbound text before a provider sees it. A literal `from` is a
    /// keyword: `stop.txt`, `Stop.txt`, and `STOP.TXT` all become `go.txt`,
    /// including inside a file. An `http` or `https` `from` value is a URL:
    /// `http://stop.com` becomes `http://go.com`, and a longer path on that
    /// URL keeps its suffix. `http://anything.else` and a lookalike host stay
    /// as written. Tools still open the real file and the real URL.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub adjustments: Vec<ContentAdjustment>,
    /// When true, each configured phrase and URL is rewritten automatically.
    /// No model chooses the replacement. Phrases become `ca-kw-01`, `ca-kw-02`,
    /// and so on. URLs become `http://ca-r01.invalid` (or `https`) and keep
    /// the rest of the path. An explicit adjustment for the same keyword wins.
    /// Tools open the original file and fetch the original URL.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub obfuscate: bool,
    /// Rewrite words in a fetched page before a provider sees that page.
    /// `latin` uses a built-in word list. `characters`, `words`, and `phrases`
    /// use `page_stand_ins`. `off` leaves the page as fetched.
    #[serde(default, skip_serializing_if = "PageStandIn::is_off")]
    pub page_stand_in: PageStandIn,
    /// Stand-in symbols for `characters`, `words`, or `phrases`.
    /// `latin` and `off` leave this empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub page_stand_ins: Vec<String>,
    /// Replace images in the outbound copy with `[image]`. The local view,
    /// disk, and tool fetches keep the real image.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub placeholder_images: bool,
    /// Words rewritten in the outbound copy. A match becomes `ca-kw-01`,
    /// `ca-kw-02`, and so on. These words do not trip. An explicit adjustment
    /// for the same word is used instead.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub obfuscation_patterns: Vec<String>,
    /// Stop entries. The same list cannot also be set with `phrases`, `urls`, or `rules`.
    #[serde(default, skip_serializing_if = "FilterStop::is_empty")]
    pub stop: FilterStop,
    /// Hide entries. The same list cannot also be set with `obfuscation_patterns` or `adjustments`.
    #[serde(default, skip_serializing_if = "FilterHide::is_empty")]
    pub hide: FilterHide,
    /// `pages = "latin"`, `pages = ["alpha", "beta"]`, or `[provider_filter.pages]`.
    #[serde(
        default,
        deserialize_with = "deserialize_pages",
        skip_serializing_if = "PagesSetting::is_absent"
    )]
    pub pages: PagesSetting,
}

/// Words and URLs that stop the session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct FilterStop {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub phrases: Vec<StopText>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub urls: Vec<StopText>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<ProviderFilterRule>,
}

impl FilterStop {
    fn is_empty(&self) -> bool {
        self.phrases.is_empty() && self.urls.is_empty() && self.rules.is_empty()
    }
}

/// A stop phrase or URL. A string is unmasked. A table can set `mask`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum StopText {
    Plain(String),
    Detailed(StopTextDetail),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StopTextDetail {
    pub text: String,
    #[serde(default)]
    pub mask: bool,
}

impl StopText {
    fn text(&self) -> &str {
        match self {
            Self::Plain(text) => text,
            Self::Detailed(detail) => &detail.text,
        }
    }

    fn mask(&self) -> bool {
        match self {
            Self::Plain(_) => false,
            Self::Detailed(detail) => detail.mask,
        }
    }
}

/// Words and URLs rewritten on the way out. These do not stop the session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct FilterHide {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub words: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pairs: Vec<ContentAdjustment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub urls: Vec<HideUrl>,
    /// Rewrite the authority of every absolute http or https URL to an opaque
    /// `ca-rNN` stand-in before the provider sees it, keeping the path, query,
    /// and fragment so the URL's shape stays visible. Path words are tokenized
    /// by the page stand-in when enabled. The local view puts each authority
    /// back. Tools still fetch the real URL.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub all_urls: bool,
}

impl FilterHide {
    fn is_empty(&self) -> bool {
        self.words.is_empty() && self.pairs.is_empty() && self.urls.is_empty() && !self.all_urls
    }
}

/// One URL rewrite. `prefix` also rewrites paths under `from`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HideUrl {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub prefix: bool,
}

/// Page rewrite. Absent leaves `page_stand_in` in charge.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PagesSetting {
    #[default]
    Absent,
    Off,
    Latin,
    Words(Vec<String>),
    Table {
        stand_in: PageStandIn,
        symbols: Vec<String>,
        images: Option<bool>,
    },
}

impl PagesSetting {
    fn is_absent(&self) -> bool {
        matches!(self, Self::Absent)
    }
}

impl Serialize for PagesSetting {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Absent | Self::Off => serializer.serialize_str("off"),
            Self::Latin => serializer.serialize_str("latin"),
            Self::Words(words) => words.serialize(serializer),
            Self::Table {
                stand_in,
                symbols,
                images,
            } => {
                let mut map = std::collections::BTreeMap::new();
                map.insert("stand_in", serde_json::json!(stand_in));
                if !symbols.is_empty() {
                    map.insert("symbols", serde_json::json!(symbols));
                }
                if let Some(images) = images {
                    map.insert("images", serde_json::json!(images));
                }
                map.serialize(serializer)
            }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PagesTable {
    #[serde(default)]
    stand_in: Option<PageStandIn>,
    #[serde(default)]
    symbols: Vec<String>,
    images: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PagesRaw {
    Mode(String),
    Words(Vec<String>),
    Table(PagesTable),
}

fn deserialize_pages<'de, D>(deserializer: D) -> Result<PagesSetting, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Option::<PagesRaw>::deserialize(deserializer)?;
    let Some(raw) = raw else {
        return Ok(PagesSetting::Absent);
    };
    match raw {
        PagesRaw::Mode(mode) => match mode.as_str() {
            "off" => Ok(PagesSetting::Off),
            "latin" => Ok(PagesSetting::Latin),
            other => Err(serde::de::Error::custom(format!(
                "provider filter pages must be \"off\", \"latin\", or a word list, not {}",
                quoted(other)
            ))),
        },
        PagesRaw::Words(words) => Ok(PagesSetting::Words(words)),
        PagesRaw::Table(table) => Ok(PagesSetting::Table {
            stand_in: table.stand_in.unwrap_or(PageStandIn::Off),
            symbols: table.symbols,
            images: table.images,
        }),
    }
}

/// How words in a fetched page are rewritten for the provider.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PageStandIn {
    #[default]
    Off,
    /// Built-in Latin words. The same English word always uses the same token.
    Latin,
    /// One or more items whose distinct characters form the alphabet.
    Characters,
    /// Distinct words, joined with a hyphen inside each token.
    Words,
    /// Distinct phrases, joined with ` / ` inside each token.
    Phrases,
}

impl PageStandIn {
    fn is_off(mode: &Self) -> bool {
        *mode == Self::Off
    }
}

/// One outbound replacement. `from` is the text or URL to change. `to` is
/// the text or URL the provider receives.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ContentAdjustment {
    pub from: String,
    pub to: String,
}

impl Default for ProviderFilterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            trip_file: None,
            phrases: Vec::new(),
            urls: Vec::new(),
            rules: Vec::new(),
            adjustments: Vec::new(),
            obfuscate: false,
            page_stand_in: PageStandIn::Off,
            page_stand_ins: Vec::new(),
            placeholder_images: false,
            obfuscation_patterns: Vec::new(),
            stop: FilterStop::default(),
            hide: FilterHide::default(),
            pages: PagesSetting::Absent,
        }
    }
}

impl ProviderFilterConfig {
    pub fn is_disabled(&self) -> bool {
        !self.enabled
            && self.trip_file.is_none()
            && self.phrases.is_empty()
            && self.urls.is_empty()
            && self.rules.is_empty()
            && self.adjustments.is_empty()
            && !self.obfuscate
            && self.page_stand_in == PageStandIn::Off
            && self.page_stand_ins.is_empty()
            && !self.placeholder_images
            && self.obfuscation_patterns.is_empty()
            && self.stop.is_empty()
            && self.hide.is_empty()
            && self.pages.is_absent()
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        with_process_filter(|| normalize(self).map(|_| ()))
    }
}

struct MaskedText {
    text: String,
    mask: bool,
    legacy: bool,
}

struct HidePair {
    from: String,
    to: String,
    /// `None` is a keyword. `Some` is a URL, and the bool rewrites child paths.
    prefix: Option<bool>,
    from_adjustments: bool,
}

struct FilterSpec {
    enabled: bool,
    trip_file: Option<PathBuf>,
    rules: Vec<ProviderFilterRule>,
    phrases: Vec<MaskedText>,
    urls: Vec<MaskedText>,
    pairs: Vec<HidePair>,
    hide_words: Vec<String>,
    hide_all_urls: bool,
    page_stand_in: PageStandIn,
    page_stand_ins: Vec<String>,
    placeholder_images: bool,
    notes: Vec<String>,
    entries: Vec<String>,
    summary: String,
}

fn quoted(value: &str) -> String {
    let mut end = value.len().min(256);
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    let value = &value[..end];
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            ch if ch.is_control() => out.push(' '),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn both_set(old_name: &str, new_name: &str, old_set: bool, new_set: bool) -> anyhow::Result<()> {
    if old_set && new_set {
        anyhow::bail!("provider filter {old_name} and {new_name} are both set");
    }
    Ok(())
}

fn normalize(config: &ProviderFilterConfig) -> anyhow::Result<FilterSpec> {
    if let Some(path) = &config.trip_file {
        if !path.is_absolute() {
            anyhow::bail!("provider filter trip_file must be an absolute path");
        }
    }
    both_set(
        "phrases",
        "stop.phrases",
        !config.phrases.is_empty(),
        !config.stop.phrases.is_empty(),
    )?;
    both_set(
        "urls",
        "stop.urls",
        !config.urls.is_empty(),
        !config.stop.urls.is_empty(),
    )?;
    both_set(
        "rules",
        "stop.rules",
        !config.rules.is_empty(),
        !config.stop.rules.is_empty(),
    )?;
    both_set(
        "obfuscation_patterns",
        "hide.words",
        !config.obfuscation_patterns.is_empty(),
        !config.hide.words.is_empty(),
    )?;
    both_set(
        "adjustments",
        "hide.pairs",
        !config.adjustments.is_empty(),
        !config.hide.pairs.is_empty(),
    )?;
    let pages_set = !config.pages.is_absent();
    if pages_set && (config.page_stand_in != PageStandIn::Off || !config.page_stand_ins.is_empty())
    {
        anyhow::bail!("provider filter page_stand_in and pages are both set");
    }
    let (page_stand_in, page_stand_ins, page_images) = resolve_pages(config)?;
    if page_images.is_some() && config.placeholder_images {
        anyhow::bail!("provider filter placeholder_images and pages.images are both set");
    }

    let mut phrases = Vec::new();
    if config.stop.phrases.is_empty() {
        for phrase in &config.phrases {
            phrases.push(MaskedText {
                text: phrase.clone(),
                mask: config.obfuscate,
                legacy: true,
            });
        }
    } else {
        for phrase in &config.stop.phrases {
            phrases.push(MaskedText {
                text: phrase.text().to_string(),
                mask: phrase.mask(),
                legacy: false,
            });
        }
    }
    let mut urls = Vec::new();
    if config.stop.urls.is_empty() {
        for url in &config.urls {
            urls.push(MaskedText {
                text: url.clone(),
                mask: config.obfuscate,
                legacy: true,
            });
        }
    } else {
        for url in &config.stop.urls {
            urls.push(MaskedText {
                text: url.text().to_string(),
                mask: url.mask(),
                legacy: false,
            });
        }
    }
    let rules = if config.stop.rules.is_empty() {
        config.rules.clone()
    } else {
        config.stop.rules.clone()
    };

    let mut pairs = Vec::new();
    if config.hide.pairs.is_empty() {
        for adjustment in &config.adjustments {
            let prefix = parse_url_rule(&adjustment.from).ok().map(|_| true);
            pairs.push(HidePair {
                from: adjustment.from.clone(),
                to: adjustment.to.clone(),
                prefix,
                from_adjustments: prefix.is_some(),
            });
        }
    } else {
        for adjustment in &config.hide.pairs {
            if stop_text_conflicts(&phrases, &urls, &rules, &adjustment.from) {
                anyhow::bail!(
                    "provider filter {} is in stop and hide",
                    quoted(&adjustment.from)
                );
            }
            pairs.push(HidePair {
                from: adjustment.from.clone(),
                to: adjustment.to.clone(),
                prefix: parse_url_rule(&adjustment.from).ok().map(|_| false),
                from_adjustments: false,
            });
        }
    }
    for url in &config.hide.urls {
        if stop_text_conflicts(&phrases, &urls, &rules, &url.from) {
            anyhow::bail!("provider filter {} is in stop and hide", quoted(&url.from));
        }
        pairs.push(HidePair {
            from: url.from.clone(),
            to: url.to.clone(),
            prefix: Some(url.prefix),
            from_adjustments: false,
        });
    }

    let hide_source_legacy = config.hide.words.is_empty();
    let mut hide_words = if hide_source_legacy {
        config.obfuscation_patterns.clone()
    } else {
        config.hide.words.clone()
    };
    let mut notes = Vec::new();
    let mut kept_words = Vec::new();
    for word in hide_words.drain(..) {
        if !hide_source_legacy
            && (urls.iter().any(|url| same_stop_text(&url.text, &word))
                || rules.iter().any(|rule| {
                    rule.kind == ProviderFilterKind::Literal && same_keyword(&rule.pattern, &word)
                }))
        {
            anyhow::bail!("provider filter {} is in stop and hide", quoted(&word));
        }
        if let Some(phrase) = phrases
            .iter_mut()
            .find(|phrase| same_keyword(&phrase.text, &word))
        {
            if phrase.legacy && hide_source_legacy {
                phrase.mask = true;
                notes.push(format!(
                    "{}: masked stop (listed in phrases and obfuscation_patterns)",
                    quoted(&phrase.text)
                ));
                continue;
            }
            anyhow::bail!("provider filter {} is in stop and hide", quoted(&word));
        }
        kept_words.push(word);
    }
    hide_words = kept_words;

    if phrases.len() + urls.len() + rules.len() > MAX_RULES {
        anyhow::bail!("provider filter accepts at most {MAX_RULES} rules");
    }
    let mut seen = std::collections::BTreeSet::new();
    for rule in &rules {
        validate_rule(rule)?;
        if !seen.insert(rule.id.clone()) {
            anyhow::bail!("provider filter rule id {} is duplicated", rule.id);
        }
    }
    for (index, phrase) in phrases.iter().enumerate() {
        validate_phrase(index, &phrase.text)?;
        let id = phrase_id(index);
        if !seen.insert(id.clone()) {
            anyhow::bail!("provider filter phrase id {id} is already used by a rule");
        }
    }
    for (index, url) in urls.iter().enumerate() {
        validate_url(index, &url.text)?;
        let id = url_id(index);
        if !seen.insert(id.clone()) {
            anyhow::bail!("provider filter url id {id} is already used by a rule");
        }
    }
    if pairs.len() > MAX_RULES {
        anyhow::bail!("provider filter accepts at most {MAX_RULES} adjustments");
    }
    for (index, pair) in pairs.iter().enumerate() {
        validate_adjustment(
            index,
            &ContentAdjustment {
                from: pair.from.clone(),
                to: pair.to.clone(),
            },
        )?;
        if pair.prefix == Some(false) && parse_url_rule(&pair.to).is_err() {
            anyhow::bail!(
                "provider filter adjustment {} to {} must be an absolute http or https URL",
                index + 1,
                quoted(&pair.to)
            );
        }
    }
    if hide_words.len() > MAX_RULES {
        anyhow::bail!("provider filter accepts at most {MAX_RULES} obfuscation patterns");
    }
    for (index, pattern) in hide_words.iter().enumerate() {
        validate_obfuscation_pattern(index, pattern)?;
    }
    validate_page_choice(page_stand_in, &page_stand_ins)?;

    let placeholder_images = page_images.unwrap_or(config.placeholder_images);
    let enabled = config.enabled || force_enabled();
    let mut spec = FilterSpec {
        enabled,
        trip_file: config.trip_file.clone(),
        rules,
        phrases,
        urls,
        pairs,
        hide_words,
        hide_all_urls: config.hide.all_urls,
        page_stand_in,
        page_stand_ins,
        placeholder_images,
        notes,
        entries: Vec::new(),
        summary: String::new(),
    };
    spec.entries = spec_entries(&spec);
    spec.summary = spec_summary(&spec);
    Ok(spec)
}

fn same_stop_text(left: &str, right: &str) -> bool {
    match (parse_url_rule(left), parse_url_rule(right)) {
        (Ok(left), Ok(right)) => url_rules_same(&left, &right),
        _ => same_keyword(left, right),
    }
}

fn stop_text_conflicts(
    phrases: &[MaskedText],
    urls: &[MaskedText],
    rules: &[ProviderFilterRule],
    text: &str,
) -> bool {
    phrases
        .iter()
        .any(|phrase| same_keyword(&phrase.text, text))
        || urls.iter().any(|url| same_stop_text(&url.text, text))
        || rules.iter().any(|rule| {
            rule.kind == ProviderFilterKind::Literal && same_keyword(&rule.pattern, text)
        })
}

fn resolve_pages(
    config: &ProviderFilterConfig,
) -> anyhow::Result<(PageStandIn, Vec<String>, Option<bool>)> {
    match &config.pages {
        PagesSetting::Absent => Ok((config.page_stand_in, config.page_stand_ins.clone(), None)),
        PagesSetting::Off => Ok((PageStandIn::Off, Vec::new(), None)),
        PagesSetting::Latin => Ok((PageStandIn::Latin, Vec::new(), None)),
        PagesSetting::Words(words) => Ok((PageStandIn::Words, words.clone(), None)),
        PagesSetting::Table {
            stand_in,
            symbols,
            images,
        } => {
            let kind = if *stand_in == PageStandIn::Off && !symbols.is_empty() {
                PageStandIn::Words
            } else {
                *stand_in
            };
            Ok((kind, symbols.clone(), *images))
        }
    }
}

fn spec_summary(spec: &FilterSpec) -> String {
    let mut parts = Vec::new();
    let words = spec.phrases.len();
    let urls = spec.urls.len();
    let rules = spec.rules.len();
    if words + urls + rules > 0 || spec.trip_file.is_some() {
        let mut stop = String::from("stop");
        let mut wrote = false;
        if words > 0 {
            stop.push_str(&format!(
                " {words} {}",
                if words == 1 { "word" } else { "words" }
            ));
            wrote = true;
        }
        if urls > 0 {
            if wrote {
                stop.push(',');
            }
            stop.push_str(&format!(
                " {urls} {}",
                if urls == 1 { "url" } else { "urls" }
            ));
            wrote = true;
        }
        if rules > 0 {
            if wrote {
                stop.push(',');
            }
            stop.push_str(&format!(
                " {rules} {}",
                if rules == 1 { "rule" } else { "rules" }
            ));
            wrote = true;
        }
        if spec.trip_file.is_some() {
            if wrote {
                stop.push(',');
            }
            stop.push_str(" trip file");
        }
        parts.push(stop);
    }
    let hide = spec.hide_words.len() + spec.pairs.len();
    if hide > 0 && spec.hide_all_urls {
        parts.push(format!("hide {hide} + urls"));
    } else if hide > 0 {
        parts.push(format!("hide {hide}"));
    } else if spec.hide_all_urls {
        parts.push("hide urls".into());
    }
    match spec.page_stand_in {
        PageStandIn::Off => {}
        PageStandIn::Latin => parts.push("pages latin".into()),
        PageStandIn::Words => parts.push(format!(
            "pages {} {}",
            spec.page_stand_ins.len(),
            if spec.page_stand_ins.len() == 1 {
                "word"
            } else {
                "words"
            }
        )),
        PageStandIn::Characters => parts.push("pages characters".into()),
        PageStandIn::Phrases => parts.push("pages phrases".into()),
    }
    if spec.placeholder_images {
        parts.push("images on".into());
    }
    parts.join(" · ")
}

fn spec_entries(spec: &FilterSpec) -> Vec<String> {
    let mut lines = Vec::new();
    for phrase in &spec.phrases {
        let kind = if parse_url_rule(&phrase.text).is_ok() {
            "substring"
        } else {
            "any case, both directions"
        };
        let mask = if phrase.mask { ", masked" } else { "" };
        lines.push(format!(
            "stop phrase {} ({kind}{mask})",
            quoted(&phrase.text)
        ));
    }
    for url in &spec.urls {
        let mask = if url.mask { ", masked" } else { "" };
        lines.push(format!("stop url {} (that url{mask})", quoted(&url.text)));
    }
    for rule in &spec.rules {
        lines.push(format!(
            "stop rule {} {} ({:?}, {:?})",
            rule.id,
            quoted(&rule.pattern),
            rule.kind,
            rule.target
        ));
    }
    for word in &spec.hide_words {
        lines.push(format!("hide word {}", quoted(word)));
    }
    for pair in &spec.pairs {
        match pair.prefix {
            None => lines.push(format!(
                "hide pair {} -> {}",
                quoted(&pair.from),
                quoted(&pair.to)
            )),
            Some(prefix) => lines.push(format!(
                "hide url {} -> {} (prefix {})",
                quoted(&pair.from),
                quoted(&pair.to),
                if prefix { "on" } else { "off" }
            )),
        }
        if pair.from_adjustments {
            lines.push(format!(
                "{}: prefix on (from adjustments)",
                quoted(&pair.from)
            ));
        }
    }
    match spec.page_stand_in {
        PageStandIn::Off => {}
        PageStandIn::Latin => lines.push("pages latin".into()),
        other => lines.push(format!("pages {other:?}")),
    }
    if spec.placeholder_images {
        lines.push("images on".into());
    }
    lines
}

fn compile_pair(index: usize, pair: &HidePair) -> anyhow::Result<CompiledAdjustment> {
    if let Some(prefix) = pair.prefix {
        let from = parse_url_rule(&pair.from).map_err(|_| {
            anyhow::anyhow!(
                "provider filter adjustment {} from must be an absolute http or https URL",
                index + 1
            )
        })?;
        let to = parse_url_rule(&pair.to).map_err(|_| {
            anyhow::anyhow!(
                "provider filter adjustment {} to must be an absolute http or https URL",
                index + 1
            )
        })?;
        return Ok(CompiledAdjustment::Url { from, to, prefix });
    }
    Ok(CompiledAdjustment::Literal {
        from: pair.from.clone(),
        to: pair.to.clone(),
    })
}

/// Automatic stand-ins for masked stops. URLs are applied before phrases so a
/// keyword does not rewrite a URL the URL rule still has to see.
fn append_masks(
    spec: &FilterSpec,
    adjustments: &mut Vec<CompiledAdjustment>,
) -> anyhow::Result<()> {
    let mut url_order: Vec<usize> = (0..spec.urls.len())
        .filter(|&index| spec.urls[index].mask)
        .collect();
    url_order.sort_by(|&left, &right| {
        let left_len = parse_url_rule(&spec.urls[left].text)
            .ok()
            .map(|rule| rule.segments.len())
            .unwrap_or(0);
        let right_len = parse_url_rule(&spec.urls[right].text)
            .ok()
            .map(|rule| rule.segments.len())
            .unwrap_or(0);
        right_len.cmp(&left_len).then(left.cmp(&right))
    });
    for index in url_order {
        let raw = &spec.urls[index].text;
        if explicit_url_covers(&spec.pairs, raw) {
            continue;
        }
        let from = parse_url_rule(raw).map_err(|_| {
            anyhow::anyhow!(
                "provider filter url {} is not an absolute http or https URL",
                index + 1
            )
        })?;
        let to = automatic_url_rule(index, &from)?;
        adjustments.push(CompiledAdjustment::Url {
            from,
            to,
            prefix: true,
        });
    }
    for (index, phrase) in spec.phrases.iter().enumerate() {
        if !phrase.mask || explicit_phrase_covers(&spec.pairs, &phrase.text) {
            continue;
        }
        adjustments.push(CompiledAdjustment::Literal {
            from: phrase.text.clone(),
            to: format!("ca-kw-{:02}", index + 1),
        });
    }
    Ok(())
}

/// Stand-ins for words that are rewritten and never trip. An explicit pair, or
/// a masked stop that already rewrote the word, keeps its token.
fn append_hide_words(
    spec: &FilterSpec,
    adjustments: &mut Vec<CompiledAdjustment>,
) -> anyhow::Result<()> {
    let mut next = 1u32;
    for pattern in &spec.hide_words {
        if explicit_phrase_covers(&spec.pairs, pattern) {
            continue;
        }
        if adjustments.iter().any(|adjustment| match adjustment {
            CompiledAdjustment::Literal { from, .. } => same_keyword(from, pattern),
            CompiledAdjustment::Url { .. } => false,
        }) {
            continue;
        }
        let to = next_keyword_stand_in(adjustments, &mut next);
        adjustments.push(CompiledAdjustment::Literal {
            from: pattern.clone(),
            to,
        });
    }
    Ok(())
}

fn format_spec(spec: &FilterSpec) -> String {
    let mut out = String::new();
    if spec.summary.is_empty() {
        if spec.enabled {
            out.push_str("filter on, nothing armed\n");
        } else {
            out.push_str("filter off\n");
        }
    } else {
        out.push_str(&spec.summary);
        out.push('\n');
    }
    for note in &spec.notes {
        out.push_str(note);
        out.push('\n');
    }
    if !spec.notes.is_empty() && !spec.entries.is_empty() {
        out.push('\n');
    }
    for entry in &spec.entries {
        out.push_str(entry);
        out.push('\n');
    }
    out
}

fn next_keyword_stand_in(adjustments: &[CompiledAdjustment], next: &mut u32) -> String {
    loop {
        let candidate = format!("ca-kw-{:02}", *next);
        *next = next.saturating_add(1);
        let taken = adjustments.iter().any(|adjustment| match adjustment {
            CompiledAdjustment::Literal { to, .. } => to == &candidate,
            CompiledAdjustment::Url { .. } => false,
        });
        if !taken {
            return candidate;
        }
    }
}

fn explicit_phrase_covers(pairs: &[HidePair], phrase: &str) -> bool {
    pairs
        .iter()
        .any(|pair| pair.prefix.is_none() && same_keyword(&pair.from, phrase))
}

fn explicit_url_covers(pairs: &[HidePair], raw: &str) -> bool {
    let Ok(wanted) = parse_url_rule(raw) else {
        return false;
    };
    pairs.iter().any(|pair| {
        parse_url_rule(&pair.from)
            .ok()
            .is_some_and(|from| url_rules_same(&from, &wanted))
    })
}

fn url_rules_same(left: &UrlRule, right: &UrlRule) -> bool {
    left.scheme == right.scheme
        && left.host == right.host
        && left.port == right.port
        && left.segments == right.segments
}

fn same_keyword(left: &str, right: &str) -> bool {
    if left.is_ascii() && right.is_ascii() {
        left.eq_ignore_ascii_case(right)
    } else {
        left.to_lowercase() == right.to_lowercase()
    }
}

fn automatic_url_rule(index: usize, from: &UrlRule) -> anyhow::Result<UrlRule> {
    let scheme = if from.scheme == "https" {
        "https"
    } else {
        "http"
    };
    let raw = format!("{scheme}://ca-r{:02}.invalid", index + 1);
    parse_url_rule(&raw).map_err(|_| {
        anyhow::anyhow!(
            "provider filter url {} could not be assigned a replacement",
            index + 1
        )
    })
}

fn phrase_id(index: usize) -> String {
    format!("phrase-{}", index + 1)
}

fn url_id(index: usize) -> String {
    format!("url-{}", index + 1)
}

fn validate_phrase(index: usize, phrase: &str) -> anyhow::Result<()> {
    if phrase.trim().is_empty() || phrase.len() > MAX_PATTERN_BYTES {
        anyhow::bail!(
            "provider filter phrase {} {} must be 1..={MAX_PATTERN_BYTES} bytes and not blank",
            index + 1,
            quoted(phrase)
        );
    }
    Ok(())
}

fn validate_obfuscation_pattern(index: usize, pattern: &str) -> anyhow::Result<()> {
    if pattern.trim().is_empty() || pattern.len() > MAX_PATTERN_BYTES {
        anyhow::bail!(
            "provider filter obfuscation pattern {} {} must be 1..={MAX_PATTERN_BYTES} bytes and not blank",
            index + 1,
            quoted(pattern)
        );
    }
    Ok(())
}

fn validate_adjustment(index: usize, adjustment: &ContentAdjustment) -> anyhow::Result<()> {
    if adjustment.from.trim().is_empty() || adjustment.from.len() > MAX_PATTERN_BYTES {
        anyhow::bail!(
            "provider filter adjustment {} from {} must be 1..={MAX_PATTERN_BYTES} bytes and not blank",
            index + 1,
            quoted(&adjustment.from)
        );
    }
    if adjustment.to.len() > MAX_PATTERN_BYTES {
        anyhow::bail!(
            "provider filter adjustment {} to must be at most {MAX_PATTERN_BYTES} bytes",
            index + 1
        );
    }
    if parse_url_rule(&adjustment.from).is_ok() && parse_url_rule(&adjustment.to).is_err() {
        anyhow::bail!(
            "provider filter adjustment {} from {} to {} must use an absolute http or https URL on both sides",
            index + 1,
            quoted(&adjustment.from),
            quoted(&adjustment.to)
        );
    }
    Ok(())
}

fn validate_url(index: usize, raw: &str) -> anyhow::Result<()> {
    if raw.trim().is_empty() || raw.len() > MAX_PATTERN_BYTES {
        anyhow::bail!(
            "provider filter url {} {} must be 1..={MAX_PATTERN_BYTES} bytes and not blank",
            index + 1,
            quoted(raw)
        );
    }
    parse_url_rule(raw).map_err(|_| {
        anyhow::anyhow!(
            "provider filter url {} {} must be an absolute http or https URL with a host",
            index + 1,
            quoted(raw)
        )
    })?;
    Ok(())
}

fn validate_rule(rule: &ProviderFilterRule) -> anyhow::Result<()> {
    if rule.id.is_empty()
        || rule.id.len() > MAX_ID_BYTES
        || !rule
            .id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        anyhow::bail!(
            "provider filter rule id must be 1..={MAX_ID_BYTES} ASCII letters, digits, '-' or '_'"
        );
    }
    if rule.pattern.is_empty() || rule.pattern.len() > MAX_PATTERN_BYTES {
        anyhow::bail!(
            "provider filter pattern for {} {} must be 1..={MAX_PATTERN_BYTES} bytes",
            rule.id,
            quoted(&rule.pattern)
        );
    }
    if rule.kind == ProviderFilterKind::Regex {
        regex::bytes::Regex::new(&rule.pattern).map_err(|error| {
            anyhow::anyhow!("provider filter regex for {} is invalid: {error}", rule.id)
        })?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Direction {
    Request,
    Response,
}

#[derive(Clone, Copy)]
enum ScanOrigin {
    Request,
    Reply,
    FetchedPage,
}

impl ScanOrigin {
    fn label(self) -> &'static str {
        match self {
            Self::Request => "request",
            Self::Reply => "reply",
            Self::FetchedPage => "fetched page",
        }
    }
}

enum Matcher {
    /// The word or phrase as written. Matching ignores case.
    Literal(String),
    Regex(regex::bytes::Regex),
    /// Scheme, host, port, and path. Query and fragment are not compared.
    Url(UrlRule),
}

struct UrlRule {
    scheme: String,
    host: String,
    port: u16,
    segments: Vec<String>,
}

impl UrlRule {
    fn matches_candidate(&self, candidate: &url::Url) -> bool {
        if candidate.scheme() != self.scheme {
            return false;
        }
        let Some(host) = candidate.host_str() else {
            return false;
        };
        if !host.eq_ignore_ascii_case(&self.host) {
            return false;
        }
        if candidate.port_or_known_default() != Some(self.port) {
            return false;
        }
        path_segments(candidate.path()) == self.segments.as_slice()
    }
}

struct CompiledRule {
    id: String,
    target: ProviderFilterTarget,
    matcher: Matcher,
    /// The text the user wrote. The local status shows it. The provider error does not.
    label: String,
}

enum TripRead {
    Absent,
    Other,
    Stop,
    Unavailable,
}

enum CompiledAdjustment {
    Literal {
        from: String,
        to: String,
    },
    Url {
        from: UrlRule,
        to: UrlRule,
        prefix: bool,
    },
}

pub struct ProviderFilter {
    enabled: bool,
    obfuscate: bool,
    obfuscation_patterns: bool,
    placeholder_images: bool,
    hide_all_urls: bool,
    /// Original URL for each `ca-rNN` stand-in, in first-seen order.
    url_restorations: Mutex<Vec<String>>,
    page: Option<PageStandInCoder>,
    rules: Vec<CompiledRule>,
    adjustments: Vec<CompiledAdjustment>,
    /// Literal adjustment replacements, longest first, left unchanged by the
    /// page stand-in so an explicit `to` still round-trips.
    protected_literals: Vec<String>,
    trip_file: Option<PathBuf>,
    tripped: AtomicBool,
    session_stop_claimed: AtomicBool,
    reason: Mutex<Option<String>>,
    local_notice: Mutex<Option<String>>,
    summary_line: String,
    report: String,
    last_trip_poll: Mutex<Option<Instant>>,
}

impl ProviderFilter {
    pub fn compile(config: &ProviderFilterConfig) -> anyhow::Result<Self> {
        with_process_filter(|| Self::compile_locked(config))
    }

    fn compile_locked(config: &ProviderFilterConfig) -> anyhow::Result<Self> {
        let spec = normalize(config)?;
        let mut rules = Vec::with_capacity(spec.rules.len() + spec.phrases.len() + spec.urls.len());
        for rule in &spec.rules {
            let matcher = match rule.kind {
                ProviderFilterKind::Literal => Matcher::Literal(rule.pattern.clone()),
                ProviderFilterKind::Regex => {
                    Matcher::Regex(regex::bytes::Regex::new(&rule.pattern).map_err(|error| {
                        anyhow::anyhow!("provider filter regex for {} is invalid: {error}", rule.id)
                    })?)
                }
            };
            rules.push(CompiledRule {
                id: rule.id.clone(),
                target: rule.target,
                matcher,
                label: rule.pattern.clone(),
            });
        }
        for (index, phrase) in spec.phrases.iter().enumerate() {
            rules.push(CompiledRule {
                id: phrase_id(index),
                target: ProviderFilterTarget::Both,
                matcher: Matcher::Literal(phrase.text.clone()),
                label: phrase.text.clone(),
            });
        }
        for (index, raw) in spec.urls.iter().enumerate() {
            rules.push(CompiledRule {
                id: url_id(index),
                target: ProviderFilterTarget::Both,
                matcher: Matcher::Url(parse_url_rule(&raw.text).map_err(|_| {
                    anyhow::anyhow!(
                        "provider filter url {} is not an absolute http or https URL",
                        index + 1
                    )
                })?),
                label: raw.text.clone(),
            });
        }
        let mut adjustments = Vec::with_capacity(spec.pairs.len());
        for (index, pair) in spec.pairs.iter().enumerate() {
            adjustments.push(compile_pair(index, pair)?);
        }
        if spec.enabled {
            append_masks(&spec, &mut adjustments)?;
            append_hide_words(&spec, &mut adjustments)?;
        }
        let page = if spec.enabled {
            PageStandInCoder::from_parts(spec.page_stand_in, &spec.page_stand_ins)?
        } else {
            None
        };
        let mut protected_literals: Vec<String> = adjustments
            .iter()
            .filter_map(|adjustment| match adjustment {
                CompiledAdjustment::Literal { to, .. } if !to.is_empty() => Some(to.clone()),
                _ => None,
            })
            .collect();
        protected_literals.sort_by_key(|item| std::cmp::Reverse(item.len()));
        protected_literals.dedup();
        let report = format_spec(&spec);
        Ok(Self {
            enabled: spec.enabled,
            obfuscate: spec.enabled && spec.phrases.iter().chain(&spec.urls).any(|item| item.mask),
            obfuscation_patterns: spec.enabled && !spec.hide_words.is_empty(),
            placeholder_images: spec.enabled && spec.placeholder_images,
            hide_all_urls: spec.enabled && spec.hide_all_urls,
            url_restorations: Mutex::new(Vec::new()),
            page,
            rules,
            adjustments,
            protected_literals,
            trip_file: spec.trip_file.clone(),
            tripped: AtomicBool::new(false),
            session_stop_claimed: AtomicBool::new(false),
            reason: Mutex::new(None),
            local_notice: Mutex::new(None),
            summary_line: spec.summary,
            report,
            last_trip_poll: Mutex::new(None),
        })
    }

    fn disabled() -> Self {
        Self {
            enabled: false,
            obfuscate: false,
            obfuscation_patterns: false,
            placeholder_images: false,
            hide_all_urls: false,
            url_restorations: Mutex::new(Vec::new()),
            page: None,
            rules: Vec::new(),
            adjustments: Vec::new(),
            protected_literals: Vec::new(),
            trip_file: None,
            tripped: AtomicBool::new(false),
            session_stop_claimed: AtomicBool::new(false),
            reason: Mutex::new(None),
            local_notice: Mutex::new(None),
            summary_line: String::new(),
            report: String::new(),
            last_trip_poll: Mutex::new(None),
        }
    }

    pub fn local_stop_notice(&self) -> Option<String> {
        self.local_notice
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
    }

    pub fn summary_line(&self) -> &str {
        &self.summary_line
    }

    pub fn report(&self) -> &str {
        &self.report
    }

    pub fn is_tripped(&self) -> bool {
        self.tripped.load(Ordering::SeqCst)
    }

    pub fn watches(&self) -> bool {
        self.enabled
            && (!self.rules.is_empty()
                || self.trip_file.is_some()
                || !self.adjustments.is_empty()
                || self.page.is_some()
                || self.placeholder_images)
    }

    /// Rewrite one outbound body, then apply request stop rules to the result.
    /// The returned bytes are what a provider may receive. Image placeholders
    /// run after keyword and page stand-ins, and the stop scan sees that result.
    pub fn prepare_request(&self, body: &[u8]) -> Result<Vec<u8>, ProviderError> {
        if !self.enabled {
            return Ok(body.to_vec());
        }
        if self.is_tripped() || self.poll_trip_file() {
            return Err(self.stopped_error());
        }
        let mut adjusted = self.adjust_bytes(body, true);
        if self.placeholder_images {
            adjusted = placeholder_image_bytes(&adjusted);
        }
        if self.hide_all_urls {
            match String::from_utf8(adjusted) {
                Ok(text) => adjusted = self.rewrite_request_urls(text).into_bytes(),
                Err(error) => adjusted = error.into_bytes(),
            }
        }
        self.scan(&adjusted, Direction::Request, ScanOrigin::Request)?;
        Ok(adjusted)
    }

    /// Scan final wire bytes after a native adapter restored immutable blocks.
    pub(crate) fn scan_prepared_request(&self, body: &[u8]) -> Result<(), ProviderError> {
        if !self.enabled {
            return Ok(());
        }
        if self.is_tripped() || self.poll_trip_file() {
            return Err(self.stopped_error());
        }
        self.scan(body, Direction::Request, ScanOrigin::Request)
    }

    /// Outbound treatment for a saved page body (local HTML/text file).
    /// Hide words and pairs apply, then page stand-in on the raw text, then
    /// image placeholders, then absolute URL stand-ins, then the stop scan.
    /// Unlike `prepare_request`, page stand-in rewrites the file body itself
    /// rather than only `web_fetch` JSON.
    pub fn prepare_page_text(&self, text: &str) -> Result<String, ProviderError> {
        if !self.enabled {
            return Ok(text.to_string());
        }
        if self.is_tripped() || self.poll_trip_file() {
            return Err(self.stopped_error());
        }
        let mut adjusted = if self.adjustments.is_empty() {
            text.to_string()
        } else {
            adjust_request_text(text, &self.adjustments)
        };
        if let Some(page) = &self.page {
            adjusted = page.encode_page(&adjusted, &self.protected_literals);
        }
        if self.placeholder_images {
            adjusted = placeholder_image_text(&adjusted);
        }
        if self.hide_all_urls {
            adjusted = self.rewrite_urls(adjusted);
        }
        self.scan(adjusted.as_bytes(), Direction::Request, ScanOrigin::Request)?;
        Ok(adjusted)
    }

    /// Tool schemas contain protocol URIs, not page content. Apply the same
    /// JSON scopes as keyword hiding before masking ordinary content URLs.
    fn rewrite_request_urls(&self, text: String) -> String {
        let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&text) else {
            return self.rewrite_urls(text);
        };
        let original = value.clone();
        let mut map = self
            .url_restorations
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        rewrite_json_text(&mut value, JsonTextScope::Protocol, false, &mut |text| {
            rewrite_all_urls(text, &mut map, self.page.as_ref())
        });
        splice_json(&text, &original, &value).unwrap_or_else(|| value.to_string())
    }

    fn rewrite_urls(&self, text: String) -> String {
        let mut map = self
            .url_restorations
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        rewrite_all_urls(&text, &mut map, self.page.as_ref())
    }

    /// Fetched HTML source is kept when a page, keyword, or image stand-in will rewrite it.
    pub fn keeps_html_source(&self) -> bool {
        self.enabled
            && (self.obfuscate
                || self.obfuscation_patterns
                || self.page.is_some()
                || self.placeholder_images)
    }

    /// Map stand-ins back to the original keyword, file name, URL, or page word.
    fn restore_text(&self, text: &str) -> String {
        if !self.enabled || text.is_empty() {
            return text.to_string();
        }
        if self.adjustments.is_empty() && self.page.is_none() && !self.hide_all_urls {
            return text.to_string();
        }
        let mut current = if self.hide_all_urls {
            self.restore_urls(text)
        } else {
            text.to_string()
        };
        if let Some(page) = &self.page {
            current = page.restore(&current);
        }
        for adjustment in self.adjustments.iter().rev() {
            current = match adjustment {
                CompiledAdjustment::Literal { from, to } => replace_literal(&current, to, from),
                CompiledAdjustment::Url { from, to, prefix } => {
                    replace_urls(&current, to, from, *prefix)
                }
            };
        }
        current
    }

    fn restore_urls(&self, text: &str) -> String {
        let map = self
            .url_restorations
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        restore_all_urls(text, &map)
    }

    fn restore_value(&self, value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::String(text) => serde_json::Value::String(self.restore_text(text)),
            serde_json::Value::Array(items) => serde_json::Value::Array(
                items.iter().map(|item| self.restore_value(item)).collect(),
            ),
            serde_json::Value::Object(map) => {
                let mut restored = serde_json::Map::new();
                for (key, item) in map {
                    restored.insert(key.clone(), self.restore_value(item));
                }
                serde_json::Value::Object(restored)
            }
            other => other.clone(),
        }
    }

    pub fn shows_banner(&self) -> bool {
        self.enabled || self.is_tripped()
    }

    /// Block one outbound body. A match or an existing trip returns before any
    /// socket write. The error names the rule id, never the matched bytes.
    #[cfg(test)]
    pub fn enforce_request(&self, body: &[u8]) -> Result<(), ProviderError> {
        self.admit_outbound(body)
    }

    /// Scan bytes gathered for a provider, including file and tool bytes that
    /// will be omitted from the excerpt. Request and `both` rules apply.
    /// Response-only rules do not. The error names the rule id.
    pub fn admit_outbound(&self, bytes: &[u8]) -> Result<(), ProviderError> {
        self.admit_direction(bytes, Direction::Request, ScanOrigin::Request)
    }

    /// Scan a fetched page on the page itself. The request is scanned again
    /// after the stand-in rewrite.
    #[cfg(test)]
    pub fn admit_fetched_page(&self, bytes: &[u8]) -> Result<(), ProviderError> {
        self.admit_direction(bytes, Direction::Request, ScanOrigin::FetchedPage)
    }

    /// Same scan as [`Self::admit_outbound`] for a stream. The carry keeps the
    /// last 8 KiB so a literal or short regex can span two reads. An empty
    /// chunk marks real EOF and decides an unterminated trailing URL.
    pub fn admit_outbound_chunk(
        &self,
        carry: &mut Vec<u8>,
        chunk: &[u8],
    ) -> Result<(), ProviderError> {
        self.admit_chunk(carry, chunk, Direction::Request, ScanOrigin::Request)
    }

    pub fn admit_fetched_page_chunk(
        &self,
        carry: &mut Vec<u8>,
        chunk: &[u8],
    ) -> Result<(), ProviderError> {
        self.admit_chunk(carry, chunk, Direction::Request, ScanOrigin::FetchedPage)
    }

    fn admit_direction(
        &self,
        bytes: &[u8],
        direction: Direction,
        origin: ScanOrigin,
    ) -> Result<(), ProviderError> {
        if !self.enabled {
            return Ok(());
        }
        if self.is_tripped() || self.poll_trip_file() {
            return Err(self.stopped_error());
        }
        match direction {
            Direction::Request => {
                let adjusted = self.adjust_bytes(bytes, false);
                self.scan(&adjusted, direction, origin)
            }
            Direction::Response => self.scan(bytes, direction, origin),
        }
    }

    fn admit_chunk(
        &self,
        carry: &mut Vec<u8>,
        chunk: &[u8],
        direction: Direction,
        origin: ScanOrigin,
    ) -> Result<(), ProviderError> {
        if !self.enabled {
            return Ok(());
        }
        if self.is_tripped() || self.poll_trip_file() {
            return Err(self.stopped_error());
        }
        let mut window = std::mem::take(carry);
        window.extend_from_slice(chunk);
        let scanned = match direction {
            Direction::Request => {
                let adjusted = self.adjust_bytes(&window, false);
                self.scan_with_url_end(&adjusted, direction, origin, chunk.is_empty())
            }
            Direction::Response => {
                self.scan_with_url_end(&window, direction, origin, chunk.is_empty())
            }
        };
        if window.len() > RESPONSE_CARRY_BYTES {
            let start = window.len() - RESPONSE_CARRY_BYTES;
            window.drain(..start);
        }
        *carry = window;
        scanned
    }

    /// Block one response chunk, including a short carry so a literal or a
    /// short regex can span two chunks. Matches longer than the 8 KiB carry
    /// must fall inside a single chunk. An empty chunk marks real EOF and
    /// decides an unterminated trailing URL; an unfinished host or path is
    /// held until a delimiter arrives so `/child` and `.evil` can extend it.
    pub fn observe_response(&self, carry: &mut Vec<u8>, chunk: &[u8]) -> Result<(), ProviderError> {
        self.admit_chunk(carry, chunk, Direction::Response, ScanOrigin::Reply)
    }

    pub async fn until_tripped(&self) {
        if !self.watches() {
            std::future::pending::<()>().await;
            return;
        }
        loop {
            if self.is_tripped() || self.poll_trip_file() {
                return;
            }
            tokio::time::sleep(TRIP_POLL_INTERVAL).await;
        }
    }

    pub fn stopped_error(&self) -> ProviderError {
        let rule = self
            .reason
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
            .unwrap_or_else(|| "provider filter".to_string());
        ProviderError::FilterStopped { rule }
    }

    fn scan(
        &self,
        bytes: &[u8],
        direction: Direction,
        origin: ScanOrigin,
    ) -> Result<(), ProviderError> {
        self.scan_with_url_end(bytes, direction, origin, true)
    }

    fn scan_with_url_end(
        &self,
        bytes: &[u8],
        direction: Direction,
        origin: ScanOrigin,
        trailing_url_complete: bool,
    ) -> Result<(), ProviderError> {
        self.scan_bytes(bytes, direction, origin, trailing_url_complete)?;
        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) {
            self.scan_json_text(&value, direction, origin, 0)?;
        }
        Ok(())
    }

    fn scan_json_text(
        &self,
        value: &serde_json::Value,
        direction: Direction,
        origin: ScanOrigin,
        depth: usize,
    ) -> Result<(), ProviderError> {
        match value {
            serde_json::Value::String(text) => {
                self.scan_bytes(text.as_bytes(), direction, origin, true)?;
                // Tool arguments may themselves be a JSON string. A bounded
                // second decode also checks their escaped string values.
                if depth < 8 {
                    if let Ok(inner) = serde_json::from_str::<serde_json::Value>(text) {
                        self.scan_json_text(&inner, direction, origin, depth + 1)?;
                    }
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    self.scan_json_text(item, direction, origin, depth)?;
                }
            }
            serde_json::Value::Object(map) => {
                for (key, item) in map {
                    self.scan_bytes(key.as_bytes(), direction, origin, true)?;
                    self.scan_json_text(item, direction, origin, depth)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn scan_bytes(
        &self,
        bytes: &[u8],
        direction: Direction,
        origin: ScanOrigin,
        trailing_url_complete: bool,
    ) -> Result<(), ProviderError> {
        let mut urls = None;
        for rule in &self.rules {
            if !rule.target.covers(direction) {
                continue;
            }
            let matched = match &rule.matcher {
                Matcher::Literal(needle) => contains_literal(bytes, needle),
                Matcher::Regex(pattern) => pattern.is_match(bytes),
                Matcher::Url(url) => {
                    let found =
                        urls.get_or_insert_with(|| extract_urls(bytes, trailing_url_complete));
                    found
                        .iter()
                        .any(|candidate| url.matches_candidate(candidate))
                }
            };
            if matched {
                return Err(self.trip_at(rule.id.as_str(), rule.label.as_str(), origin));
            }
        }
        Ok(())
    }

    fn trip(&self, rule: &str) -> ProviderError {
        self.trip_at(rule, rule, ScanOrigin::Request)
    }

    fn trip_at(&self, rule: &str, label: &str, origin: ScanOrigin) -> ProviderError {
        self.reason
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .get_or_insert_with(|| rule.to_string());
        let notice = if rule == "trip file" || rule == "trip file unavailable" {
            format!("stopped on \"{rule}\"")
        } else {
            format!("stopped on \"{label}\", in the {}", origin.label())
        };
        self.local_notice
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .get_or_insert_with(|| notice);
        self.tripped.store(true, Ordering::SeqCst);
        self.stopped_error()
    }

    fn poll_trip_file(&self) -> bool {
        if self.is_tripped() {
            return true;
        }
        let Some(path) = &self.trip_file else {
            return false;
        };
        if !self.enabled {
            return false;
        }
        {
            let mut last = self
                .last_trip_poll
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            if last.is_some_and(|instant| instant.elapsed() < TRIP_POLL_INTERVAL) {
                return false;
            }
            *last = Some(Instant::now());
        }
        match read_trip_file(path) {
            TripRead::Stop => {
                let _ = self.trip("trip file");
                true
            }
            TripRead::Unavailable => {
                let _ = self.trip("trip file unavailable");
                true
            }
            TripRead::Absent | TripRead::Other => false,
        }
    }

    fn adjust_bytes(&self, bytes: &[u8], provider_body: bool) -> Vec<u8> {
        if self.adjustments.is_empty() && self.page.is_none() {
            return bytes.to_vec();
        }
        let Some((prefix_len, text, suffix)) = adjustable_span(bytes) else {
            return bytes.to_vec();
        };
        if text.is_empty() {
            return bytes.to_vec();
        }
        let mut adjusted = if self.adjustments.is_empty() {
            text.to_string()
        } else if provider_body {
            adjust_request_text(text, &self.adjustments)
        } else {
            adjust_text(text, &self.adjustments)
        };
        if provider_body {
            if let Some(page) = &self.page {
                adjusted = page.rewrite_outbound(&adjusted, &self.protected_literals);
            }
        }
        if adjusted == text {
            return bytes.to_vec();
        }
        let mut out = Vec::with_capacity(prefix_len + adjusted.len() + suffix.len());
        out.extend_from_slice(&bytes[..prefix_len]);
        out.extend_from_slice(adjusted.as_bytes());
        out.extend_from_slice(suffix);
        out
    }
}

fn parse_url_rule(raw: &str) -> Result<UrlRule, ()> {
    let parsed = url::Url::parse(raw.trim()).map_err(|_| ())?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err(());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(());
    }
    let Some(host) = parsed.host_str() else {
        return Err(());
    };
    if host.is_empty() {
        return Err(());
    }
    let Some(port) = parsed.port_or_known_default() else {
        return Err(());
    };
    Ok(UrlRule {
        scheme: parsed.scheme().to_string(),
        host: host.to_ascii_lowercase(),
        port,
        segments: path_segments(parsed.path()),
    })
}

fn path_segments(path: &str) -> Vec<String> {
    // Keep distinct interior segments. One trailing slash retains the existing
    // root/path equivalence, but `/a//b` is a different path from `/a/b`.
    if path.is_empty() || path == "/" {
        return Vec::new();
    }
    let repeated_tail = path.ends_with("//");
    let path = path.strip_prefix('/').unwrap_or(path);
    let path = if repeated_tail {
        path
    } else {
        path.strip_suffix('/').unwrap_or(path)
    };
    path.split('/').map(str::to_string).collect()
}

fn extract_urls(haystack: &[u8], trailing_complete: bool) -> Vec<url::Url> {
    let mut found = Vec::new();
    let mut index = 0;
    while index < haystack.len() {
        let Some((offset, scheme_len)) = find_http_scheme(&haystack[index..]) else {
            break;
        };
        let start = index + offset;
        let token_len = url_token_len(&haystack[start..]);
        if let Ok(token) = std::str::from_utf8(&haystack[start..start + token_len]) {
            let tail = &haystack[start + token_len..];
            let incomplete_utf8_tail = !tail.is_empty()
                && std::str::from_utf8(tail)
                    .is_err_and(|error| error.valid_up_to() == 0 && error.error_len().is_none());
            let token_is_unterminated = tail.is_empty() || incomplete_utf8_tail;
            // Query and fragment start after the path. Their continuation
            // cannot change this URL's comparison identity, so a long query
            // does not need to keep the URL's beginning in the 8 KiB carry.
            if !trailing_complete && token_is_unterminated && !token.contains(['?', '#']) {
                index = start + scheme_len;
                continue;
            }
            let trimmed = trim_url_tail(token);
            if let Ok(parsed) = url::Url::parse(trimmed) {
                if parsed.host_str().is_some() {
                    found.push(parsed);
                }
            }
        }
        index = start + scheme_len;
    }
    found
}

fn find_http_scheme(bytes: &[u8]) -> Option<(usize, usize)> {
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index].eq_ignore_ascii_case(&b'h') {
            if has_ascii_prefix(&bytes[index..], b"https://") {
                return Some((index, 8));
            }
            if has_ascii_prefix(&bytes[index..], b"http://") {
                return Some((index, 7));
            }
        }
        index += 1;
    }
    None
}

fn has_ascii_prefix(bytes: &[u8], prefix: &[u8]) -> bool {
    bytes.len() >= prefix.len()
        && bytes
            .iter()
            .zip(prefix)
            .all(|(byte, expected)| byte.eq_ignore_ascii_case(expected))
}

fn url_token_len(bytes: &[u8]) -> usize {
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte.is_ascii() {
            if !is_url_byte(byte) {
                break;
            }
            index += 1;
            continue;
        }
        let width = match byte {
            0xC2..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF4 => 4,
            _ => break,
        };
        let Some(character) = bytes
            .get(index..index + width)
            .and_then(|part| std::str::from_utf8(part).ok())
            .and_then(|part| part.chars().next())
        else {
            break;
        };
        if character.is_whitespace()
            || character.is_control()
            || matches!(character, '«' | '»' | '“' | '”' | '‘' | '’')
        {
            break;
        }
        index += width;
    }
    index
}

fn is_url_byte(byte: u8) -> bool {
    matches!(
        byte,
        b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'.'
            | b'_'
            | b'~'
            | b':'
            | b'/'
            | b'?'
            | b'#'
            | b'['
            | b']'
            | b'@'
            | b'!'
            | b'$'
            | b'&'
            | b'\''
            | b'('
            | b')'
            | b'*'
            | b'+'
            | b','
            | b';'
            | b'='
            | b'%'
    )
}

/// Drop sentence punctuation and a closing wrapper that is not part of the URL.
fn trim_url_tail(token: &str) -> &str {
    let mut end = token.len();
    while end > 0 {
        let Some(ch) = token[..end].chars().next_back() else {
            break;
        };
        if matches!(
            ch,
            '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '"' | '>' | '}'
        ) {
            end -= ch.len_utf8();
            continue;
        }
        if ch == ')' || ch == ']' {
            let (left, right) = if ch == ')' {
                (b'(', b')')
            } else {
                (b'[', b']')
            };
            let open = token[..end].bytes().filter(|byte| *byte == left).count();
            let close = token[..end].bytes().filter(|byte| *byte == right).count();
            if close > open {
                end -= 1;
                continue;
            }
        }
        break;
    }
    &token[..end]
}

/// Split an absolute http(s) URL into its redacted authority key
/// (`scheme://host[:port]`, userinfo dropped, scheme lowercased) and the
/// remaining path/query/fragment. Returns `None` when the URL has no host.
fn split_url_authority(trimmed: &str) -> Option<(String, &str)> {
    let parsed = url::Url::parse(trimmed).ok()?;
    let host = parsed.host_str()?;
    if host.is_empty() {
        return None;
    }
    let scheme_len = if trimmed.len() >= 8 && trimmed[..8].eq_ignore_ascii_case("https://") {
        8
    } else {
        7
    };
    let scheme = if scheme_len == 8 { "https" } else { "http" };
    let after_scheme = &trimmed[scheme_len..];
    let authority_end = after_scheme
        .bytes()
        .position(|byte| matches!(byte, b'/' | b'?' | b'#'))
        .unwrap_or(after_scheme.len());
    let raw_authority = &after_scheme[..authority_end];
    let host_port = match raw_authority.rfind('@') {
        Some(at) => &raw_authority[at + 1..],
        None => raw_authority,
    };
    if host_port.is_empty() {
        return None;
    }
    let authority = format!("{scheme}://{host_port}");
    let path = &trimmed[scheme_len + raw_authority.len()..];
    Some((authority, path))
}

/// Replace the authority of every absolute http(s) URL with an opaque
/// `scheme://ca-rNN.invalid` stand-in, keeping the path, query, and fragment
/// so the URL's shape stays visible to the provider. When a page stand-in
/// coder is supplied, path words are tokenized by it (like relative links),
/// so slugs do not leak. The same authority keeps the same `NN`. `map`
/// records authorities in stand-in order for the local view. Userinfo is
/// dropped, and trailing punctuation is left in place.
fn rewrite_all_urls(text: &str, map: &mut Vec<String>, coder: Option<&PageStandInCoder>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some((offset, scheme_len)) = find_http_scheme(rest.as_bytes()) {
        out.push_str(&rest[..offset]);
        let token_len = url_token_len(&rest.as_bytes()[offset..]);
        if token_len == 0 {
            out.push_str(&rest[offset..offset + 1]);
            rest = &rest[offset + 1..];
            continue;
        }
        let token = &rest[offset..offset + token_len];
        let trimmed = trim_url_tail(token);
        let keep = &token[trimmed.len()..];
        let Some((authority, path)) = split_url_authority(trimmed) else {
            out.push_str(token);
            rest = &rest[offset + token_len..];
            continue;
        };
        let scheme = if scheme_len == 8 { "https" } else { "http" };
        let existing = map
            .iter()
            .position(|item| item.as_str() == authority.as_str());
        let number = match existing {
            Some(index) => index + 1,
            None => {
                map.push(authority);
                map.len()
            }
        };
        out.push_str(&format!("{scheme}://ca-r{number:02}.invalid"));
        match coder {
            Some(coder) => out.push_str(&coder.encode_words_only(path)),
            None => out.push_str(path),
        }
        out.push_str(keep);
        rest = &rest[offset + token_len..];
    }
    out.push_str(rest);
    out
}

fn restore_all_urls(text: &str, map: &[String]) -> String {
    if map.is_empty() {
        return text.to_string();
    }
    let mut out = text.to_string();
    // Longest stand-in numbers last so `ca-r10` is not eaten by `ca-r1`.
    for (index, authority) in map.iter().enumerate() {
        let number = index + 1;
        for scheme in ["https://", "http://"] {
            let stand_in = format!("{scheme}ca-r{number:02}.invalid");
            out = replace_literal(&out, &stand_in, authority);
        }
    }
    out
}

/// A UTF-8 file read can split a character across the 8 KiB carry. Adjust the
/// valid text and keep a short broken edge. Bytes that are not text stay as
/// they are.
fn adjustable_span(bytes: &[u8]) -> Option<(usize, &str, &[u8])> {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Some((0, text, &[]));
    }
    let mut start = 0;
    while start < bytes.len() && start < 3 && is_utf8_continuation(bytes[start]) {
        start += 1;
    }
    if start == bytes.len() {
        return None;
    }
    match std::str::from_utf8(&bytes[start..]) {
        Ok(text) => Some((start, text, &[])),
        Err(error) if error.error_len().is_none() => {
            let valid = error.valid_up_to();
            let text = std::str::from_utf8(&bytes[start..start + valid]).ok()?;
            Some((start, text, &bytes[start + valid..]))
        }
        Err(_) => None,
    }
}

fn is_utf8_continuation(byte: u8) -> bool {
    byte & 0b1100_0000 == 0b1000_0000
}

/// Rewrite decoded content, preserving JSON keys and provider identifiers.
/// Escaping replacements after the rewrite keeps quotes and newlines valid.
fn adjust_request_text(text: &str, adjustments: &[CompiledAdjustment]) -> String {
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(text) else {
        return adjust_text(text, adjustments);
    };
    let original = value.clone();
    let provider_body = value.as_object().is_some_and(|map| {
        [
            "messages",
            "input",
            "content",
            "prompt",
            "system",
            "instructions",
            "tools",
        ]
        .iter()
        .any(|key| map.contains_key(*key))
    });
    let scope = if provider_body {
        JsonTextScope::Protocol
    } else {
        JsonTextScope::Arguments
    };
    adjust_json_value(&mut value, adjustments, scope);
    splice_json(text, &original, &value)
        .unwrap_or_else(|| serde_json::to_string(&value).unwrap_or_else(|_| text.to_string()))
}

#[derive(Clone, Copy)]
enum JsonTextScope {
    Protocol,
    Content,
    Arguments,
    Schema,
    SchemaMap,
}

fn adjust_json_value(
    value: &mut serde_json::Value,
    adjustments: &[CompiledAdjustment],
    scope: JsonTextScope,
) {
    rewrite_json_text(value, scope, true, &mut |text| {
        adjust_text(text, adjustments)
    });
}

fn rewrite_json_text(
    value: &mut serde_json::Value,
    scope: JsonTextScope,
    protect_images: bool,
    rewrite: &mut impl FnMut(&str) -> String,
) {
    match value {
        serde_json::Value::String(text) => {
            let image_data = protect_images
                && !matches!(scope, JsonTextScope::Arguments)
                && starts_with_ignore(text.as_bytes(), 0, b"data:image/");
            if !image_data && !matches!(scope, JsonTextScope::Schema | JsonTextScope::SchemaMap) {
                *text = rewrite(text);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                rewrite_json_text(item, scope, protect_images, rewrite);
            }
        }
        serde_json::Value::Object(map) => {
            // Native image blocks contain opaque image bytes or a fetchable
            // image URL. Text substitutions would corrupt that media. The
            // optional image placeholder pass still runs afterwards.
            let kind = map.get("type").and_then(serde_json::Value::as_str);
            if protect_images
                && !matches!(
                    scope,
                    JsonTextScope::Arguments | JsonTextScope::Schema | JsonTextScope::SchemaMap
                )
                && matches!(kind, Some("image" | "image_url" | "input_image"))
            {
                return;
            }
            let tool_input =
                map.get("type").and_then(serde_json::Value::as_str) == Some("tool_use");
            for (key, item) in map {
                if matches!(scope, JsonTextScope::Arguments) {
                    rewrite_json_text(item, scope, protect_images, rewrite);
                    continue;
                }
                if matches!(scope, JsonTextScope::SchemaMap) {
                    rewrite_json_text(item, JsonTextScope::Schema, protect_images, rewrite);
                    continue;
                }
                if matches!(scope, JsonTextScope::Schema) {
                    let next = match key.as_str() {
                        "const" | "enum" | "default" | "examples" => continue,
                        "properties" | "patternProperties" | "$defs" | "definitions"
                        | "dependentSchemas" | "dependencies" => JsonTextScope::SchemaMap,
                        "description" | "title" if item.is_string() => JsonTextScope::Content,
                        _ => JsonTextScope::Schema,
                    };
                    rewrite_json_text(item, next, protect_images, rewrite);
                    continue;
                }
                match key.as_str() {
                    "arguments" => {
                        if let serde_json::Value::String(text) = item {
                            if let Ok(mut arguments) =
                                serde_json::from_str::<serde_json::Value>(text)
                            {
                                let original = arguments.clone();
                                rewrite_json_text(
                                    &mut arguments,
                                    JsonTextScope::Arguments,
                                    protect_images,
                                    rewrite,
                                );
                                *text = splice_json(text, &original, &arguments)
                                    .unwrap_or_else(|| arguments.to_string());
                            } else {
                                *text = rewrite(text);
                            }
                        } else {
                            rewrite_json_text(
                                item,
                                JsonTextScope::Arguments,
                                protect_images,
                                rewrite,
                            );
                        }
                    }
                    "input" if tool_input => {
                        rewrite_json_text(item, JsonTextScope::Arguments, protect_images, rewrite);
                    }
                    "metadata" => {
                        // Metadata keys are user labels, not protocol fields.
                        rewrite_json_text(item, JsonTextScope::Arguments, protect_images, rewrite);
                    }
                    "format" | "tool_choice" => {
                        // A scalar selects a protocol mode. Object variants
                        // may also carry descriptions and a named schema.
                        if item.is_object() || item.is_array() {
                            rewrite_json_text(
                                item,
                                JsonTextScope::Protocol,
                                protect_images,
                                rewrite,
                            );
                        }
                    }
                    "parameters" | "input_schema" | "schema" => {
                        rewrite_json_text(item, JsonTextScope::Schema, protect_images, rewrite);
                    }
                    "content" | "text" | "output" | "instructions" | "system" | "prompt"
                    | "description" | "reasoning" | "reasoning_content" | "thinking"
                    | "summary" | "image" => {
                        rewrite_json_text(item, JsonTextScope::Content, protect_images, rewrite);
                    }
                    "model"
                    | "role"
                    | "type"
                    | "name"
                    | "id"
                    | "call_id"
                    | "tool_call_id"
                    | "tool_use_id"
                    | "media_type"
                    | "encoding"
                    | "status"
                    | "previous_response_id"
                    | "size"
                    | "reasoning_effort"
                    | "effort"
                    | "verbosity"
                    | "service_tier"
                    | "truncation"
                    | "include"
                    | "modalities"
                    | "mode"
                    | "prompt_cache_key"
                    | "encrypted_content" => {}
                    _ => {
                        rewrite_json_text(item, scope, protect_images, rewrite);
                    }
                }
            }
        }
        _ => {}
    }
}

fn adjust_text(text: &str, adjustments: &[CompiledAdjustment]) -> String {
    let mut current = text.to_string();
    for adjustment in adjustments {
        current = match adjustment {
            CompiledAdjustment::Literal { from, to } => replace_literal(&current, from, to),
            CompiledAdjustment::Url { from, to, prefix } => {
                replace_urls(&current, from, to, *prefix)
            }
        };
    }
    current
}

/// Replace a detected keyword before the provider sees it.
///
/// Phrase detection ignores case, so the same keyword is changed in any case.
/// The inserted text is not scanned again.
fn replace_literal(text: &str, from: &str, to: &str) -> String {
    if from.is_empty() {
        return text.to_string();
    }
    if from.is_ascii() {
        return replace_ascii_keyword(text, from.as_bytes(), to);
    }
    replace_unicode_keyword(text, from, to)
}

fn replace_ascii_keyword(text: &str, needle: &[u8], to: &str) -> String {
    let haystack = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < haystack.len() {
        if index + needle.len() <= haystack.len()
            && haystack[index..index + needle.len()]
                .iter()
                .zip(needle)
                .all(|(byte, expected)| byte.eq_ignore_ascii_case(expected))
        {
            out.push_str(to);
            index += needle.len();
            continue;
        }
        let Some(ch) = text[index..].chars().next() else {
            break;
        };
        out.push(ch);
        index += ch.len_utf8();
    }
    out
}

fn replace_unicode_keyword(text: &str, from: &str, to: &str) -> String {
    let folded = from.to_lowercase();
    let lower = text.to_lowercase();
    let mut boundaries = vec![(0, 0)];
    let mut folded_offset = 0;
    for (offset, ch) in text.char_indices() {
        folded_offset += ch.to_lowercase().map(char::len_utf8).sum::<usize>();
        boundaries.push((folded_offset, offset + ch.len_utf8()));
    }
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    for (start, _) in lower.match_indices(&folded) {
        let end = start + folded.len();
        let start_slot = boundaries
            .partition_point(|(at, _)| *at <= start)
            .saturating_sub(1);
        let end_slot = boundaries.partition_point(|(at, _)| *at < end);
        let Some((_, original_end)) = boundaries.get(end_slot) else {
            continue;
        };
        let original_start = boundaries[start_slot].1;
        if original_start < copied {
            continue;
        }
        out.push_str(&text[copied..original_start]);
        out.push_str(to);
        copied = *original_end;
    }
    out.push_str(&text[copied..]);
    out
}

fn replace_urls(text: &str, from: &UrlRule, to: &UrlRule, prefix: bool) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < bytes.len() {
        let Some((offset, scheme_len)) = find_http_scheme(&bytes[index..]) else {
            out.push_str(&text[index..]);
            break;
        };
        let start = index + offset;
        out.push_str(&text[index..start]);
        let token_len = url_token_len(&bytes[start..]);
        let token = &text[start..start + token_len];
        let trimmed = trim_url_tail(token);
        if let Some(rewritten) = rewrite_url(trimmed, from, to, prefix) {
            out.push_str(&rewritten);
            out.push_str(&token[trimmed.len()..]);
            index = start + token_len;
        } else {
            out.push_str(&text[start..start + scheme_len]);
            index = start + scheme_len;
        }
    }
    out
}

fn rewrite_url(token: &str, from: &UrlRule, to: &UrlRule, prefix: bool) -> Option<String> {
    let parsed = url::Url::parse(token).ok()?;
    if parsed.scheme() != from.scheme {
        return None;
    }
    let host = parsed.host_str()?;
    if !host.eq_ignore_ascii_case(&from.host) {
        return None;
    }
    if parsed.port_or_known_default() != Some(from.port) {
        return None;
    }
    let segments = path_segments(parsed.path());
    if !prefix && segments.len() != from.segments.len() {
        return None;
    }
    if segments.len() < from.segments.len() || segments[..from.segments.len()] != from.segments[..]
    {
        return None;
    }
    let suffix = raw_suffix_after_prefix(token, &from.segments)?;
    Some(join_rewritten(to, suffix))
}

fn raw_suffix_after_prefix<'a>(token: &'a str, prefix: &[String]) -> Option<&'a str> {
    let marker = token.find("://")?;
    let rest = &token[marker + 3..];
    let authority = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let tail = &token[marker + 3 + authority..];
    if prefix.is_empty() {
        return Some(tail);
    }
    let mut index = 0;
    let bytes = tail.as_bytes();
    for expected in prefix {
        if index >= bytes.len() || bytes[index] != b'/' {
            return None;
        }
        index += 1;
        let segment_start = index;
        while index < bytes.len()
            && bytes[index] != b'/'
            && bytes[index] != b'?'
            && bytes[index] != b'#'
        {
            index += 1;
        }
        if index == segment_start && !expected.is_empty() {
            return None;
        }
        if !segment_matches(&tail[segment_start..index], expected) {
            return None;
        }
    }
    Some(&tail[index..])
}

fn segment_matches(raw: &str, expected: &str) -> bool {
    raw == expected || decode_percent(raw) == decode_percent(expected)
}

fn decode_percent(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return None;
            }
            let hi = hex_value(bytes[index + 1])?;
            let lo = hex_value(bytes[index + 2])?;
            out.push((hi << 4) | lo);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn join_rewritten(to: &UrlRule, suffix: &str) -> String {
    let mut out = render_url(to);
    out.push_str(suffix);
    out
}

fn render_url(rule: &UrlRule) -> String {
    let mut out = String::new();
    out.push_str(&rule.scheme);
    out.push_str("://");
    if rule.host.contains(':') && !rule.host.starts_with('[') {
        out.push('[');
        out.push_str(&rule.host);
        out.push(']');
    } else {
        out.push_str(&rule.host);
    }
    let default_port = if rule.scheme == "https" { 443 } else { 80 };
    if rule.port != default_port {
        out.push(':');
        out.push_str(&rule.port.to_string());
    }
    for segment in &rule.segments {
        out.push('/');
        push_path_segment(&mut out, segment);
    }
    out
}

fn push_path_segment(out: &mut String, segment: &str) {
    if segment.bytes().all(is_raw_segment_byte) {
        out.push_str(segment);
        return;
    }
    for byte in segment.bytes() {
        if is_raw_segment_byte(byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
}

fn is_raw_segment_byte(byte: u8) -> bool {
    is_url_byte(byte) && !matches!(byte, b'/' | b'?' | b'#')
}

fn contains_literal(haystack: &[u8], needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    if needle.is_ascii() {
        let needle = needle.as_bytes();
        return haystack.len() >= needle.len()
            && haystack.windows(needle.len()).any(|window| {
                window
                    .iter()
                    .zip(needle)
                    .all(|(hay, needle)| hay.eq_ignore_ascii_case(needle))
            });
    }
    let needle = needle.to_lowercase();
    String::from_utf8_lossy(haystack)
        .to_lowercase()
        .contains(&needle)
}

fn read_trip_file(path: &Path) -> TripRead {
    let file = match open_trip_file(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return TripRead::Absent,
        Err(_) => return TripRead::Unavailable,
    };
    let metadata = match file.metadata() {
        Ok(metadata) => metadata,
        Err(_) => return TripRead::Unavailable,
    };
    if !metadata.is_file() {
        return TripRead::Unavailable;
    }
    if metadata.len() > MAX_TRIP_FILE_BYTES {
        return TripRead::Other;
    }
    let mut text = String::new();
    if file
        .take(MAX_TRIP_FILE_BYTES)
        .read_to_string(&mut text)
        .is_err()
    {
        return TripRead::Unavailable;
    }
    if text.trim() == "stop" {
        TripRead::Stop
    } else {
        TripRead::Other
    }
}

#[cfg(unix)]
fn open_trip_file(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
}

#[cfg(not(unix))]
fn open_trip_file(path: &Path) -> std::io::Result<std::fs::File> {
    if path
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::FilesystemLoop,
            "provider filter trip file must be a regular file",
        ));
    }
    std::fs::File::open(path)
}

const WEB_FETCH_TOOL: &str = "web_fetch";
const PAGE_TOKEN_OPEN: char = '«';
const PAGE_TOKEN_CLOSE: char = '»';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageSymbolKind {
    Characters,
    Words,
    Phrases,
}

#[derive(Debug, Clone)]
struct PageStandInCoder {
    kind: PageSymbolKind,
    alphabet: Vec<String>,
    index: std::collections::BTreeMap<String, usize>,
    symbols_per_byte: usize,
}

fn validate_page_choice(kind: PageStandIn, symbols: &[String]) -> anyhow::Result<()> {
    match kind {
        PageStandIn::Off => {
            if !symbols.is_empty() {
                anyhow::bail!(
                    "provider filter page_stand_in off does not accept page_stand_ins {}",
                    quoted(&symbols.join(", "))
                );
            }
        }
        PageStandIn::Latin => {
            if !symbols.is_empty() {
                anyhow::bail!(
                    "provider filter page_stand_in latin does not accept page_stand_ins {}",
                    quoted(&symbols.join(", "))
                );
            }
        }
        PageStandIn::Characters => {
            character_alphabet(symbols)?;
        }
        PageStandIn::Words => {
            symbol_alphabet(symbols, false)?;
        }
        PageStandIn::Phrases => {
            symbol_alphabet(symbols, true)?;
        }
    }
    Ok(())
}

fn latin_alphabet() -> Vec<String> {
    [
        "aqua", "terra", "ignis", "aer", "lux", "nox", "sol", "luna", "stella", "mons", "flumen",
        "silva", "urbs", "via", "porta", "domus", "rex", "lex", "pax", "fides", "spes", "amor",
        "vita", "mors", "corpus", "manus", "oculus", "cor", "vox", "verbum", "liber", "tempus",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn character_alphabet(items: &[String]) -> anyhow::Result<Vec<String>> {
    if items.is_empty() {
        anyhow::bail!("provider filter page stand-in needs at least two distinct characters");
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut alphabet = Vec::new();
    for (index, item) in items.iter().enumerate() {
        if item.is_empty() {
            anyhow::bail!(
                "provider filter page stand-in item {} {} is invalid",
                index + 1,
                quoted(item)
            );
        }
        for ch in item.chars() {
            if !allowed_stand_in_char(ch) {
                anyhow::bail!(
                    "provider filter page stand-in item {} {} is invalid",
                    index + 1,
                    quoted(item)
                );
            }
            if seen.insert(ch) {
                alphabet.push(ch.to_string());
            }
        }
    }
    if alphabet.len() < 2 {
        anyhow::bail!("provider filter page stand-in needs at least two distinct characters");
    }
    Ok(alphabet)
}

fn symbol_alphabet(items: &[String], phrases: bool) -> anyhow::Result<Vec<String>> {
    if items.len() < 2 {
        anyhow::bail!(
            "provider filter page stand-in needs at least two distinct {}",
            if phrases { "phrases" } else { "words" }
        );
    }
    let mut seen = std::collections::BTreeSet::new();
    for (index, item) in items.iter().enumerate() {
        if !stand_in_symbol_ok(item, phrases) || !seen.insert(item.as_str()) {
            anyhow::bail!(
                "provider filter page stand-in item {} {} is invalid",
                index + 1,
                quoted(item)
            );
        }
    }
    Ok(items.to_vec())
}

fn allowed_stand_in_char(ch: char) -> bool {
    ch != '"'
        && ch != '\\'
        && ch != PAGE_TOKEN_OPEN
        && ch != PAGE_TOKEN_CLOSE
        && !ch.is_whitespace()
        && !ch.is_control()
}

fn stand_in_symbol_ok(item: &str, phrases: bool) -> bool {
    if item.is_empty()
        || item.chars().any(|ch| {
            ch == '"'
                || ch == '\\'
                || ch == PAGE_TOKEN_OPEN
                || ch == PAGE_TOKEN_CLOSE
                || ch.is_control()
        })
    {
        return false;
    }
    if phrases {
        !item.contains(" / ")
    } else {
        !item
            .chars()
            .any(|ch| ch.is_whitespace() || ch == '-' || ch == '/')
    }
}

impl PageStandInCoder {
    fn from_parts(kind: PageStandIn, symbols: &[String]) -> anyhow::Result<Option<Self>> {
        let (kind, alphabet) = match kind {
            PageStandIn::Off => return Ok(None),
            PageStandIn::Latin => (PageSymbolKind::Words, latin_alphabet()),
            PageStandIn::Characters => (PageSymbolKind::Characters, character_alphabet(symbols)?),
            PageStandIn::Words => (PageSymbolKind::Words, symbol_alphabet(symbols, false)?),
            PageStandIn::Phrases => (PageSymbolKind::Phrases, symbol_alphabet(symbols, true)?),
        };
        Ok(Some(Self::new(kind, alphabet)))
    }

    fn new(kind: PageSymbolKind, alphabet: Vec<String>) -> Self {
        let index = alphabet
            .iter()
            .enumerate()
            .map(|(slot, symbol)| (symbol.clone(), slot))
            .collect();
        let symbols_per_byte = symbols_per_byte(alphabet.len());
        Self {
            kind,
            alphabet,
            index,
            symbols_per_byte,
        }
    }

    /// Rewrite `web_fetch` page text inside a provider request. Other tool
    /// output, the user's own words, and JSON keys stay as written. A body
    /// that is not a request is left alone, so a fetch-time stop scan still
    /// sees the page.
    fn rewrite_outbound(&self, text: &str, protected: &[String]) -> String {
        let trimmed = text.trim_start();
        if !trimmed.starts_with('{') && !trimmed.starts_with('[') {
            return text.to_string();
        }
        let Ok(mut value) = serde_json::from_str::<serde_json::Value>(text) else {
            return text.to_string();
        };
        let mut ids = std::collections::BTreeSet::new();
        collect_fetch_ids(&value, &mut ids);
        if ids.is_empty() {
            return text.to_string();
        }
        let original = value.clone();
        if !encode_fetch_pages(&mut value, &ids, self, protected) {
            return text.to_string();
        }
        splice_json(text, &original, &value)
            .unwrap_or_else(|| serde_json::to_string(&value).unwrap_or_else(|_| text.to_string()))
    }

    fn restore(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(start) = rest.find(PAGE_TOKEN_OPEN) {
            out.push_str(&rest[..start]);
            let after = &rest[start + PAGE_TOKEN_OPEN.len_utf8()..];
            if let Some(end) = after.find(PAGE_TOKEN_CLOSE) {
                if let Some(word) = self.decode_inner(&after[..end]) {
                    out.push_str(&word);
                    rest = &after[end + PAGE_TOKEN_CLOSE.len_utf8()..];
                    continue;
                }
            }
            out.push(PAGE_TOKEN_OPEN);
            rest = after;
        }
        out.push_str(rest);
        out
    }

    fn encode_page(&self, text: &str, protected: &[String]) -> String {
        let mut out = String::with_capacity(text.len());
        let mut index = 0;
        while index < text.len() {
            if let Some(len) = protected_at(text, index, protected) {
                out.push_str(&text[index..index + len]);
                index += len;
                continue;
            }
            if text[index..].starts_with(PAGE_TOKEN_OPEN) {
                let open = PAGE_TOKEN_OPEN.len_utf8();
                if let Some(end) = text[index + open..].find(PAGE_TOKEN_CLOSE) {
                    let close_at = index + open + end;
                    let inner = &text[index + open..close_at];
                    let token_end = close_at + PAGE_TOKEN_CLOSE.len_utf8();
                    if self.decode_inner(inner).is_some() {
                        out.push_str(&text[index..token_end]);
                        index = token_end;
                        continue;
                    }
                }
            }
            if let Some(len) = url_token_len_at(text, index) {
                out.push_str(&text[index..index + len]);
                index += len;
                continue;
            }
            if text[index..].starts_with("<!--") {
                out.push_str("<!--");
                index += 4;
                if let Some(end) = text[index..].find("-->") {
                    out.push_str(&self.encode_page(&text[index..index + end], protected));
                    out.push_str("-->");
                    index += end + 3;
                } else {
                    out.push_str(&self.encode_page(&text[index..], protected));
                    break;
                }
                continue;
            }
            if text.as_bytes()[index] == b'<' {
                index = self.copy_tag(text, index, protected, &mut out);
                continue;
            }
            if let Some(len) = entity_len(&text[index..]) {
                out.push_str(&text[index..index + len]);
                index += len;
                continue;
            }
            if let Some(len) = word_at(&text[index..]) {
                out.push_str(&self.encode_word(&text[index..index + len]));
                index += len;
                continue;
            }
            let ch = text[index..].chars().next().unwrap_or('\u{FFFD}');
            out.push(ch);
            index += ch.len_utf8();
        }
        out
    }

    fn copy_tag(&self, text: &str, start: usize, protected: &[String], out: &mut String) -> usize {
        out.push('<');
        let mut index = start + 1;
        while index < text.len() {
            let byte = text.as_bytes()[index];
            if byte == b'>' {
                out.push('>');
                return index + 1;
            }
            if byte == b'"' || byte == b'\'' {
                let quote = byte as char;
                let value_start = index + 1;
                if let Some(end) = text[value_start..].find(quote) {
                    let value = &text[value_start..value_start + end];
                    out.push(quote);
                    if value_is_url(value)
                        || is_structural_attr(&attribute_name_before(text, index))
                    {
                        out.push_str(value);
                    } else {
                        out.push_str(&self.encode_page(value, protected));
                    }
                    out.push(quote);
                    index = value_start + end + quote.len_utf8();
                    continue;
                }
            }
            let ch = text[index..].chars().next().unwrap_or('\u{FFFD}');
            out.push(ch);
            index += ch.len_utf8();
        }
        index
    }

    fn encode_word(&self, word: &str) -> String {
        let radix = self.alphabet.len();
        let width = self.symbols_per_byte;
        let mut digits = Vec::with_capacity(word.len() * width + 1);
        for byte in word.as_bytes() {
            let mut value = usize::from(*byte);
            let mut slots = vec![0; width];
            for slot in (0..width).rev() {
                slots[slot] = value % radix;
                value /= radix;
            }
            digits.extend(slots);
        }
        let sum = word
            .as_bytes()
            .iter()
            .fold(0usize, |acc, byte| acc.wrapping_add(usize::from(*byte)));
        digits.push(sum % radix);
        self.frame(&digits)
    }

    fn frame(&self, digits: &[usize]) -> String {
        let mut out = String::new();
        out.push(PAGE_TOKEN_OPEN);
        match self.kind {
            PageSymbolKind::Characters => {
                for digit in digits {
                    out.push_str(&self.alphabet[*digit]);
                }
            }
            PageSymbolKind::Words => {
                for (index, digit) in digits.iter().enumerate() {
                    if index > 0 {
                        out.push('-');
                    }
                    out.push_str(&self.alphabet[*digit]);
                }
            }
            PageSymbolKind::Phrases => {
                for (index, digit) in digits.iter().enumerate() {
                    if index > 0 {
                        out.push_str(" / ");
                    }
                    out.push_str(&self.alphabet[*digit]);
                }
            }
        }
        out.push(PAGE_TOKEN_CLOSE);
        out
    }

    /// Tokenize every word (alphabetic run) in a URL path/query/fragment,
    /// preserving punctuation, digits, and separators. Used so absolute URL
    /// paths get the same stand-in treatment as relative links once the
    /// authority has been replaced.
    fn encode_words_only(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut index = 0;
        while index < text.len() {
            if let Some(len) = word_at(&text[index..]) {
                out.push_str(&self.encode_word(&text[index..index + len]));
                index += len;
                continue;
            }
            let ch = text[index..].chars().next().unwrap_or('\u{FFFD}');
            out.push(ch);
            index += ch.len_utf8();
        }
        out
    }

    fn decode_inner(&self, inner: &str) -> Option<String> {
        if inner.is_empty() {
            return None;
        }
        let mut digits = Vec::new();
        match self.kind {
            PageSymbolKind::Characters => {
                for ch in inner.chars() {
                    digits.push(*self.index.get(&ch.to_string())?);
                }
            }
            PageSymbolKind::Words => {
                for symbol in inner.split('-') {
                    digits.push(*self.index.get(symbol)?);
                }
            }
            PageSymbolKind::Phrases => {
                for symbol in inner.split(" / ") {
                    digits.push(*self.index.get(symbol)?);
                }
            }
        }
        if digits.len() < 2 {
            return None;
        }
        let check = digits.pop()?;
        let bytes = decode_bytes(&digits, self.alphabet.len(), self.symbols_per_byte)?;
        let sum = bytes
            .iter()
            .fold(0usize, |acc, byte| acc.wrapping_add(usize::from(*byte)));
        if sum % self.alphabet.len() != check {
            return None;
        }
        let word = std::str::from_utf8(&bytes).ok()?;
        is_one_word(word).then(|| word.to_string())
    }
}

fn symbols_per_byte(radix: usize) -> usize {
    let mut count = 1usize;
    let mut space = radix.max(1);
    while space < 256 {
        count += 1;
        if radix <= 1 || space > usize::MAX / radix {
            break;
        }
        space *= radix;
    }
    count
}

fn decode_bytes(digits: &[usize], radix: usize, width: usize) -> Option<Vec<u8>> {
    if digits.is_empty() || width == 0 || !digits.len().is_multiple_of(width) {
        return None;
    }
    let mut bytes = Vec::with_capacity(digits.len() / width);
    for chunk in digits.chunks(width) {
        let mut value = 0usize;
        for digit in chunk {
            if *digit >= radix {
                return None;
            }
            value = value.saturating_mul(radix).saturating_add(*digit);
        }
        if value > 255 {
            return None;
        }
        bytes.push(value as u8);
    }
    Some(bytes)
}

fn protected_at(text: &str, index: usize, protected: &[String]) -> Option<usize> {
    let rest = &text[index..];
    protected.iter().find_map(|item| {
        (!item.is_empty() && rest.starts_with(item.as_str())).then_some(item.len())
    })
}

fn url_token_len_at(text: &str, index: usize) -> Option<usize> {
    let rest = &text.as_bytes()[index..];
    if has_ascii_prefix(rest, b"https://") || has_ascii_prefix(rest, b"http://") {
        Some(url_token_len(rest))
    } else {
        None
    }
}

fn value_is_url(value: &str) -> bool {
    let trimmed = value.trim();
    let bytes = trimmed.as_bytes();
    has_ascii_prefix(bytes, b"https://")
        || has_ascii_prefix(bytes, b"http://")
        || trimmed.contains("://")
}

/// Attribute names that carry page structure rather than content. Their
/// values are kept verbatim so selectors (`class`), canonical/pagination
/// links (`rel`), and `#id` anchors survive the page stand-in.
fn is_structural_attr(name: &str) -> bool {
    matches!(name, "class" | "rel" | "id")
}

/// Return the attribute name immediately before the quote at `quote_index`
/// (skipping the `=` and whitespace), lowercased for comparison.
fn attribute_name_before(text: &str, quote_index: usize) -> String {
    let bytes = text.as_bytes();
    let mut i = quote_index;
    while i > 0 && bytes[i - 1].is_ascii_whitespace() {
        i -= 1;
    }
    while i > 0 && (bytes[i - 1] == b'=' || bytes[i - 1].is_ascii_whitespace()) {
        i -= 1;
    }
    let mut end = i;
    while end > 0 {
        let byte = bytes[end - 1];
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':' | b'.') {
            end -= 1;
        } else {
            break;
        }
    }
    text[end..i].to_ascii_lowercase()
}

fn entity_len(text: &str) -> Option<usize> {
    let rest = text.strip_prefix('&')?;
    let end = rest.find(';')?;
    if end == 0 || end > 16 {
        return None;
    }
    let name = &rest[..end];
    let ok = if let Some(numeric) = name.strip_prefix('#') {
        let digits = numeric.strip_prefix(['x', 'X']).unwrap_or(numeric);
        !digits.is_empty() && digits.chars().all(|ch| ch.is_ascii_hexdigit())
    } else {
        name.chars().all(|ch| ch.is_ascii_alphanumeric())
    };
    ok.then_some(1 + end + 1)
}

fn word_at(text: &str) -> Option<usize> {
    let mut chars = text.chars();
    let first = chars.next()?;
    if !first.is_alphabetic() {
        return None;
    }
    let mut len = first.len_utf8();
    while len < text.len() {
        let ch = text[len..].chars().next()?;
        if ch.is_alphabetic() {
            len += ch.len_utf8();
            continue;
        }
        if ch == '\'' || ch == '-' {
            let after = len + ch.len_utf8();
            let next = text.get(after..)?.chars().next()?;
            if next.is_alphabetic() {
                len = after + next.len_utf8();
                continue;
            }
        }
        break;
    }
    Some(len)
}

fn is_one_word(text: &str) -> bool {
    word_at(text).is_some_and(|len| len == text.len())
}

fn collect_fetch_ids(value: &serde_json::Value, ids: &mut std::collections::BTreeSet<String>) {
    match value {
        serde_json::Value::Array(items) => {
            for item in items {
                collect_fetch_ids(item, ids);
            }
        }
        serde_json::Value::Object(map) => {
            note_fetch_call(map, ids);
            for item in map.values() {
                collect_fetch_ids(item, ids);
            }
        }
        _ => {}
    }
}

fn note_fetch_call(
    map: &serde_json::Map<String, serde_json::Value>,
    ids: &mut std::collections::BTreeSet<String>,
) {
    if let Some(function) = map.get("function").and_then(|value| value.as_object()) {
        if function.get("name").and_then(|value| value.as_str()) == Some(WEB_FETCH_TOOL) {
            if let Some(id) = map.get("id").and_then(|value| value.as_str()) {
                ids.insert(id.to_string());
            }
        }
    }
    if map.get("name").and_then(|value| value.as_str()) == Some(WEB_FETCH_TOOL) {
        let kind = map.get("type").and_then(|value| value.as_str());
        if matches!(kind, Some("function_call") | Some("tool_use")) {
            if let Some(id) = map
                .get("call_id")
                .or_else(|| map.get("id"))
                .and_then(|value| value.as_str())
            {
                ids.insert(id.to_string());
            }
        }
    }
}

fn is_fetch_result(
    map: &serde_json::Map<String, serde_json::Value>,
    ids: &std::collections::BTreeSet<String>,
) -> bool {
    if map.get("role").and_then(|value| value.as_str()) == Some("tool") {
        if let Some(id) = map.get("tool_call_id").and_then(|value| value.as_str()) {
            return ids.contains(id);
        }
    }
    let kind = map.get("type").and_then(|value| value.as_str());
    if kind == Some("function_call_output") {
        if let Some(id) = map.get("call_id").and_then(|value| value.as_str()) {
            return ids.contains(id);
        }
    }
    if kind == Some("tool_result") {
        if let Some(id) = map.get("tool_use_id").and_then(|value| value.as_str()) {
            return ids.contains(id);
        }
    }
    false
}

fn encode_fetch_pages(
    value: &mut serde_json::Value,
    ids: &std::collections::BTreeSet<String>,
    coder: &PageStandInCoder,
    protected: &[String],
) -> bool {
    match value {
        serde_json::Value::Array(items) => {
            let mut changed = false;
            for item in items {
                changed |= encode_fetch_pages(item, ids, coder, protected);
            }
            changed
        }
        serde_json::Value::Object(map) => {
            let fetch = is_fetch_result(map, ids);
            let mut changed = false;
            if fetch {
                changed |= encode_result_text(map, coder, protected);
            }
            for (key, item) in map.iter_mut() {
                if fetch && matches!(key.as_str(), "content" | "output") {
                    continue;
                }
                changed |= encode_fetch_pages(item, ids, coder, protected);
            }
            changed
        }
        _ => false,
    }
}

fn encode_result_text(
    map: &mut serde_json::Map<String, serde_json::Value>,
    coder: &PageStandInCoder,
    protected: &[String],
) -> bool {
    let mut changed = false;
    for key in ["content", "output"] {
        if let Some(value) = map.get_mut(key) {
            changed |= encode_page_value(value, coder, protected);
        }
    }
    changed
}

fn encode_page_value(
    value: &mut serde_json::Value,
    coder: &PageStandInCoder,
    protected: &[String],
) -> bool {
    match value {
        serde_json::Value::String(text) => {
            let encoded = coder.encode_page(text, protected);
            if encoded == *text {
                false
            } else {
                *text = encoded;
                true
            }
        }
        serde_json::Value::Array(items) => {
            let mut changed = false;
            for item in items {
                changed |= encode_block(item, coder, protected);
            }
            changed
        }
        _ => false,
    }
}

fn encode_block(
    value: &mut serde_json::Value,
    coder: &PageStandInCoder,
    protected: &[String],
) -> bool {
    match value {
        serde_json::Value::String(_) => encode_page_value(value, coder, protected),
        serde_json::Value::Object(map) => {
            if map.get("type").and_then(|value| value.as_str()) != Some("text") {
                return false;
            }
            let Some(serde_json::Value::String(text)) = map.get_mut("text") else {
                return false;
            };
            let encoded = coder.encode_page(text, protected);
            if encoded == *text {
                return false;
            }
            *text = encoded;
            true
        }
        _ => false,
    }
}

const IMAGE_PLACEHOLDER: &str = "[image]";

/// Replace image blocks and image markup. Unchanged text keeps its original bytes.
fn placeholder_image_bytes(bytes: &[u8]) -> Vec<u8> {
    let Some((prefix_len, text, suffix)) = adjustable_span(bytes) else {
        return bytes.to_vec();
    };
    let replaced = placeholder_image_text(text);
    if replaced == text {
        return bytes.to_vec();
    }
    let mut out = Vec::with_capacity(prefix_len + replaced.len() + suffix.len());
    out.extend_from_slice(&bytes[..prefix_len]);
    out.extend_from_slice(replaced.as_bytes());
    out.extend_from_slice(suffix);
    out
}

fn placeholder_image_text(text: &str) -> String {
    let trimmed = text.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(text) {
            let original = value.clone();
            if !placeholder_image_value(&mut value) {
                return text.to_string();
            }
            return splice_json(text, &original, &value).unwrap_or_else(|| {
                serde_json::to_string(&value).unwrap_or_else(|_| text.to_string())
            });
        }
    }
    placeholder_in_text(text)
}

fn placeholder_image_value(value: &mut serde_json::Value) -> bool {
    if let Some(kind) = value.as_object().and_then(image_block_kind) {
        *value = serde_json::json!({
            "type": kind,
            "text": IMAGE_PLACEHOLDER,
        });
        return true;
    }
    match value {
        serde_json::Value::Array(items) => {
            let mut changed = false;
            for item in items {
                changed |= placeholder_image_value(item);
            }
            changed
        }
        serde_json::Value::Object(map) => {
            let mut changed = false;
            for item in map.values_mut() {
                changed |= placeholder_image_value(item);
            }
            changed
        }
        serde_json::Value::String(text) => {
            let replaced = placeholder_in_text(text);
            if replaced == *text {
                false
            } else {
                *text = replaced;
                true
            }
        }
        _ => false,
    }
}

/// Chat `image_url` and Anthropic `image` become `text`. Responses
/// `input_image` becomes `input_text`. A bare `type` without image bytes stays.
fn image_block_kind(map: &serde_json::Map<String, serde_json::Value>) -> Option<&'static str> {
    let kind = map.get("type").and_then(|value| value.as_str())?;
    match kind {
        "image_url" if map.contains_key("image_url") => Some("text"),
        "input_image" if map.contains_key("image_url") => Some("input_text"),
        "image" if map.contains_key("source") || has_image_media(map) => Some("text"),
        _ => None,
    }
}

fn has_image_media(map: &serde_json::Map<String, serde_json::Value>) -> bool {
    map.get("media_type")
        .and_then(|value| value.as_str())
        .is_some_and(|media| {
            media.len() >= 6 && media.as_bytes()[..6].eq_ignore_ascii_case(b"image/")
        })
        || map.contains_key("image_url")
}

fn placeholder_in_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    let mut changed = false;
    while index < text.len() {
        if let Some(len) = image_tag_len(text, index)
            .or_else(|| markdown_image_len(text, index))
            .or_else(|| data_image_len(text, index))
        {
            out.push_str(IMAGE_PLACEHOLDER);
            index += len;
            changed = true;
            continue;
        }
        let ch = text[index..].chars().next().unwrap_or('\u{FFFD}');
        out.push(ch);
        index += ch.len_utf8();
    }
    if changed {
        out
    } else {
        text.to_string()
    }
}

/// `<image` is checked before `<img`. An unclosed tag is not a match.
fn image_tag_len(text: &str, index: usize) -> Option<usize> {
    let rest = text.get(index..)?;
    let bytes = rest.as_bytes();
    if bytes.first() != Some(&b'<') {
        return None;
    }
    let name_at = if bytes.get(1) == Some(&b'/') { 2 } else { 1 };
    let name_len = if starts_with_ignore(bytes, name_at, b"image") {
        5
    } else if starts_with_ignore(bytes, name_at, b"img") {
        3
    } else {
        return None;
    };
    let after = name_at + name_len;
    if !bytes
        .get(after)
        .is_some_and(|byte| byte.is_ascii_whitespace() || matches!(byte, b'/' | b'>'))
    {
        return None;
    }
    let mut cursor = after;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'"' | b'\'' => {
                let quote = bytes[cursor];
                cursor += 1;
                while cursor < bytes.len() && bytes[cursor] != quote {
                    cursor += 1;
                }
                if cursor >= bytes.len() {
                    return None;
                }
                cursor += 1;
            }
            b'>' => return Some(cursor + 1),
            _ => cursor += 1,
        }
    }
    None
}

/// `![alt](url)` on one line. A missing close leaves the rest of the page.
fn markdown_image_len(text: &str, index: usize) -> Option<usize> {
    let rest = text.get(index..)?;
    if !rest.starts_with("![") {
        return None;
    }
    let line_end = rest.find('\n').unwrap_or(rest.len());
    let line = &rest[..line_end];
    let alt_end = line[2..].find(']')?;
    let bracket = 2 + alt_end;
    if !line[bracket..].starts_with("](") {
        return None;
    }
    let url_at = bracket + 2;
    let close = line[url_at..].find(')')?;
    Some(url_at + close + 1)
}

fn data_image_len(text: &str, index: usize) -> Option<usize> {
    let rest = text.get(index..)?;
    let bytes = rest.as_bytes();
    if !starts_with_ignore(bytes, 0, b"data:image/") {
        return None;
    }
    let mut cursor = "data:image/".len();
    while cursor < bytes.len() && !data_image_ends(bytes[cursor]) {
        cursor += 1;
    }
    Some(cursor)
}

fn data_image_ends(byte: u8) -> bool {
    byte.is_ascii_whitespace() || matches!(byte, b'"' | b'\'' | b')' | b'<' | b'>' | b']')
}

fn starts_with_ignore(bytes: &[u8], at: usize, prefix: &[u8]) -> bool {
    bytes
        .get(at..at + prefix.len())
        .is_some_and(|window| window.eq_ignore_ascii_case(prefix))
}

fn splice_json(raw: &str, before: &serde_json::Value, after: &serde_json::Value) -> Option<String> {
    let mut index = 0;
    let mut out = String::with_capacity(raw.len());
    splice_value(raw, &mut index, before, after, &mut out)?;
    out.push_str(&raw[index..]);
    Some(out)
}

fn splice_value(
    raw: &str,
    index: &mut usize,
    before: &serde_json::Value,
    after: &serde_json::Value,
    out: &mut String,
) -> Option<()> {
    let ws = *index;
    skip_json_ws(raw, index);
    out.push_str(&raw[ws..*index]);
    match (before, after) {
        (serde_json::Value::String(left), serde_json::Value::String(right)) => {
            if raw.as_bytes().get(*index) != Some(&b'"') {
                return None;
            }
            let start = *index;
            skip_json_string(raw, index);
            if left == right {
                out.push_str(&raw[start..*index]);
            } else {
                out.push_str(&serde_json::to_string(right).ok()?);
            }
            Some(())
        }
        (serde_json::Value::Array(left), serde_json::Value::Array(right))
            if left.len() == right.len() =>
        {
            if raw.as_bytes().get(*index) != Some(&b'[') {
                return None;
            }
            out.push('[');
            *index += 1;
            for (slot, (left_item, right_item)) in left.iter().zip(right.iter()).enumerate() {
                let ws = *index;
                skip_json_ws(raw, index);
                out.push_str(&raw[ws..*index]);
                if slot > 0 {
                    if raw.as_bytes().get(*index) != Some(&b',') {
                        return None;
                    }
                    out.push(',');
                    *index += 1;
                    let ws = *index;
                    skip_json_ws(raw, index);
                    out.push_str(&raw[ws..*index]);
                }
                splice_value(raw, index, left_item, right_item, out)?;
            }
            let ws = *index;
            skip_json_ws(raw, index);
            out.push_str(&raw[ws..*index]);
            if raw.as_bytes().get(*index) != Some(&b']') {
                return None;
            }
            out.push(']');
            *index += 1;
            Some(())
        }
        (serde_json::Value::Object(left), serde_json::Value::Object(right)) => {
            if left.len() == right.len() && left.keys().all(|key| right.contains_key(key)) {
                splice_object(raw, index, left, right, out)
            } else {
                let start = *index;
                skip_json_value(raw, index);
                if *index == start {
                    return None;
                }
                out.push_str(&serde_json::to_string(right).ok()?);
                Some(())
            }
        }
        _ => {
            if before != after {
                return None;
            }
            let start = *index;
            skip_json_value(raw, index);
            if *index == start {
                return None;
            }
            out.push_str(&raw[start..*index]);
            Some(())
        }
    }
}

fn splice_object(
    raw: &str,
    index: &mut usize,
    left: &serde_json::Map<String, serde_json::Value>,
    right: &serde_json::Map<String, serde_json::Value>,
    out: &mut String,
) -> Option<()> {
    if raw.as_bytes().get(*index) != Some(&b'{') {
        return None;
    }
    out.push('{');
    *index += 1;
    let mut first = true;
    loop {
        let ws = *index;
        skip_json_ws(raw, index);
        out.push_str(&raw[ws..*index]);
        match raw.as_bytes().get(*index) {
            Some(b'}') => {
                out.push('}');
                *index += 1;
                return Some(());
            }
            Some(b',') if !first => {
                out.push(',');
                *index += 1;
                let ws = *index;
                skip_json_ws(raw, index);
                out.push_str(&raw[ws..*index]);
            }
            Some(b'"') if first => {}
            _ => return None,
        }
        first = false;
        let key_start = *index;
        if raw.as_bytes().get(*index) != Some(&b'"') {
            return None;
        }
        skip_json_string(raw, index);
        let key_raw = &raw[key_start..*index];
        let key: String = serde_json::from_str(key_raw).ok()?;
        out.push_str(key_raw);
        let ws = *index;
        skip_json_ws(raw, index);
        out.push_str(&raw[ws..*index]);
        if raw.as_bytes().get(*index) != Some(&b':') {
            return None;
        }
        out.push(':');
        *index += 1;
        splice_value(raw, index, left.get(&key)?, right.get(&key)?, out)?;
    }
}

fn skip_json_ws(raw: &str, index: &mut usize) {
    let bytes = raw.as_bytes();
    while bytes
        .get(*index)
        .is_some_and(|byte| byte.is_ascii_whitespace())
    {
        *index += 1;
    }
}

fn skip_json_string(raw: &str, index: &mut usize) {
    let bytes = raw.as_bytes();
    if bytes.get(*index) != Some(&b'"') {
        return;
    }
    *index += 1;
    while *index < bytes.len() {
        match bytes[*index] {
            b'\\' => {
                *index += 1;
                if *index >= bytes.len() {
                    return;
                }
                if bytes[*index] == b'u' {
                    *index = (*index + 5).min(bytes.len());
                } else {
                    *index += 1;
                }
            }
            b'"' => {
                *index += 1;
                return;
            }
            _ => *index += 1,
        }
    }
}

fn skip_json_value(raw: &str, index: &mut usize) {
    let Some(byte) = raw.as_bytes().get(*index).copied() else {
        return;
    };
    match byte {
        b'"' => skip_json_string(raw, index),
        b'{' => skip_json_container(raw, index, b'{', b'}'),
        b'[' => skip_json_container(raw, index, b'[', b']'),
        _ => {
            while raw.as_bytes().get(*index).is_some_and(|byte| {
                !matches!(byte, b',' | b'}' | b']') && !byte.is_ascii_whitespace()
            }) {
                *index += 1;
            }
        }
    }
}

fn skip_json_container(raw: &str, index: &mut usize, open: u8, close: u8) {
    let bytes = raw.as_bytes();
    *index += 1;
    let mut depth = 1usize;
    while *index < bytes.len() && depth > 0 {
        match bytes[*index] {
            b'"' => skip_json_string(raw, index),
            byte if byte == open => {
                depth += 1;
                *index += 1;
            }
            byte if byte == close => {
                depth -= 1;
                *index += 1;
            }
            _ => *index += 1,
        }
    }
}

fn process_slot() -> &'static RwLock<Arc<ProviderFilter>> {
    static SLOT: OnceLock<RwLock<Arc<ProviderFilter>>> = OnceLock::new();
    SLOT.get_or_init(|| RwLock::new(Arc::new(ProviderFilter::disabled())))
}

pub fn current() -> Arc<ProviderFilter> {
    process_slot()
        .read()
        .unwrap_or_else(|poison| poison.into_inner())
        .clone()
}

#[cfg(test)]
pub(crate) fn with_test_filter<T>(config: &ProviderFilterConfig, body: impl FnOnce() -> T) -> T {
    with_process_filter(|| {
        struct Restore {
            filter: Arc<ProviderFilter>,
            notice: Option<String>,
            force: bool,
            skip: bool,
        }
        impl Drop for Restore {
            fn drop(&mut self) {
                *process_slot()
                    .write()
                    .unwrap_or_else(|poison| poison.into_inner()) = Arc::clone(&self.filter);
                *notice_slot()
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner()) = self.notice.clone();
                force_flag().store(self.force, Ordering::SeqCst);
                skip_install_flag().store(self.skip, Ordering::SeqCst);
            }
        }
        let _restore = Restore {
            filter: current(),
            notice: notice_slot()
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .clone(),
            force: force_flag().load(Ordering::SeqCst),
            skip: skip_install_flag().load(Ordering::SeqCst),
        };
        force_flag().store(false, Ordering::SeqCst);
        skip_install_flag().store(false, Ordering::SeqCst);
        *process_slot()
            .write()
            .unwrap_or_else(|poison| poison.into_inner()) =
            Arc::new(ProviderFilter::compile(config).expect("test filter compiles"));
        body()
    })
}

thread_local! {
    static PROCESS_FILTER_LOCK_HELD: Cell<bool> = const { Cell::new(false) };
}

fn process_filter_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// Serialize process-slot changes with reads of the force flag. The same
/// thread may re-enter. A test holds this lock until it restores the slot.
fn with_process_filter<T>(body: impl FnOnce() -> T) -> T {
    if PROCESS_FILTER_LOCK_HELD.with(Cell::get) {
        return body();
    }
    let _guard = process_filter_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    PROCESS_FILTER_LOCK_HELD.with(|held| held.set(true));
    struct Release;
    impl Drop for Release {
        fn drop(&mut self) {
            PROCESS_FILTER_LOCK_HELD.with(|held| held.set(false));
        }
    }
    let _release = Release;
    body()
}

fn force_flag() -> &'static AtomicBool {
    static FLAG: OnceLock<AtomicBool> = OnceLock::new();
    FLAG.get_or_init(|| AtomicBool::new(false))
}

fn skip_install_flag() -> &'static AtomicBool {
    static FLAG: OnceLock<AtomicBool> = OnceLock::new();
    FLAG.get_or_init(|| AtomicBool::new(false))
}

fn notice_slot() -> &'static Mutex<Option<String>> {
    static SLOT: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

fn force_enabled() -> bool {
    force_flag().load(Ordering::SeqCst)
}

/// `ca --filter` and `/filter on` arm the configured filter for this process
/// and do not write the file.
pub fn force_process_enabled() {
    force_flag().store(true, Ordering::SeqCst);
}

/// `ca filter check` reads the file without installing the process filter.
pub fn suspend_install() {
    skip_install_flag().store(true, Ordering::SeqCst);
}

pub fn take_process_notice() -> Option<String> {
    notice_slot()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .take()
}

fn set_notice(text: impl Into<String>) {
    *notice_slot()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner()) = Some(text.into());
}

/// Install the user's filter. The first enabled snapshot sticks for this
/// process. A later config load cannot disable it or replace its rules, and a
/// trip is never cleared. Restart ca to edit rules.
pub fn install(config: &ProviderFilterConfig) -> anyhow::Result<()> {
    with_process_filter(|| install_locked(config))
}

fn install_locked(config: &ProviderFilterConfig) -> anyhow::Result<()> {
    let next = ProviderFilter::compile(config)?;
    if skip_install_flag().load(Ordering::SeqCst) {
        return Ok(());
    }
    let mut slot = process_slot()
        .write()
        .unwrap_or_else(|poison| poison.into_inner());
    if !should_replace(&slot) {
        if slot.report != next.report {
            set_notice("filter file changed; this process is still on the copy from startup.");
        }
        return Ok(());
    }
    let startup = if next.enabled && !next.watches() {
        Some("filter on, nothing armed".to_string())
    } else if next.watches() {
        Some(format!(
            "filter loaded: {}. Restart ca to change it.",
            next.summary_line
        ))
    } else {
        None
    };
    *slot = Arc::new(next);
    if let Some(startup) = startup {
        set_notice(startup);
    }
    Ok(())
}

pub fn is_tripped() -> bool {
    current().is_tripped()
}

/// The interactive banner is on while this process is enforcing the filter.
pub fn banner_active() -> bool {
    current().shows_banner()
}

/// The second banner row. After a trip this is the local stop line.
/// An enabled filter with empty lists says `nothing armed`.
pub fn banner_detail() -> String {
    let filter = current();
    if filter.is_tripped() {
        if let Some(notice) = filter.local_stop_notice() {
            return notice;
        }
    }
    if filter.summary_line().is_empty() && filter.enabled {
        return "nothing armed".to_string();
    }
    filter.summary_line().to_string()
}

/// `/filter` pager. While this process is off, the list includes `/filter on`.
pub fn command_report(filter: &ProviderFilter) -> String {
    let report = filter.report();
    let body = if report.trim().is_empty() {
        "filter off\n"
    } else {
        report
    };
    if filter.enabled || filter.is_tripped() {
        return body.to_string();
    }
    let mut out = String::new();
    if !body.starts_with("filter off") {
        out.push_str("filter off\n");
    }
    out.push_str(body);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("/filter on turns it on for this process.\n");
    out
}

/// Arm the loaded filter for this process. Does not write the config file.
/// The first enabled snapshot sticks. A trip stays stuck.
pub fn arm_loaded(config: &ProviderFilterConfig) -> anyhow::Result<String> {
    with_process_filter(|| arm_loaded_locked(config))
}

fn arm_loaded_locked(config: &ProviderFilterConfig) -> anyhow::Result<String> {
    let active = current();
    if active.is_tripped() {
        let notice = active
            .local_stop_notice()
            .unwrap_or_else(|| "This session is stopped.".to_string());
        return Ok(format!("{notice} This process stays stopped."));
    }
    if active.enabled {
        let summary = active.summary_line();
        return Ok(if summary.is_empty() {
            "filter already on, nothing armed".to_string()
        } else {
            format!("filter already on: {summary}.")
        });
    }
    let was_forced = force_enabled();
    force_process_enabled();
    match install(config) {
        Ok(()) => Ok(take_process_notice().unwrap_or_else(|| "filter on".to_string())),
        Err(error) => {
            if !was_forced {
                force_flag().store(false, Ordering::SeqCst);
            }
            Err(error)
        }
    }
}

/// Armed summary, compatibility notes, and an optional sample. Does not install.
pub fn format_check(config: &ProviderFilterConfig, text: Option<&str>) -> anyhow::Result<String> {
    with_process_filter(|| format_check_locked(config, text))
}

/// Same report as `format_check`, but the sample is a saved page body and is
/// rewritten with page stand-in on the raw text. Does not install.
pub fn format_check_page(config: &ProviderFilterConfig, text: &str) -> anyhow::Result<String> {
    with_process_filter(|| {
        let spec = normalize(config)?;
        let mut out = format_spec(&spec);
        let filter = ProviderFilter::compile(config)?;
        match filter.prepare_page_text(text) {
            Ok(sent) => {
                out.push_str("would send: ");
                out.push_str(&truncate_check_body(&sent));
                out.push('\n');
            }
            Err(ProviderError::FilterStopped { .. }) => {
                push_stop_line(&filter, &mut out);
            }
            Err(error) => anyhow::bail!("{error}"),
        }
        Ok(out)
    })
}

fn truncate_check_body(text: &str) -> String {
    if text.len() <= OUTBOUND_PANE_TEXT_LIMIT {
        return text.to_string();
    }
    let mut end = OUTBOUND_PANE_TEXT_LIMIT;
    while !text.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    let mut out = text[..end].to_string();
    out.push_str("\n\n[truncated]");
    out
}

fn push_stop_line(filter: &ProviderFilter, out: &mut String) {
    if let Some(notice) = filter.local_stop_notice() {
        out.push_str("would ");
        if let Some(rest) = notice.strip_prefix("stopped") {
            out.push_str("stop");
            out.push_str(rest);
        } else {
            out.push_str(&notice);
        }
        out.push('\n');
    }
}

/// One-run CLI overlay for `ca filter check`. Mutates a clone of the loaded
/// config only; never writes the config file. Any flag arms the preview.
pub fn apply_cli_overlay(
    config: &mut ProviderFilterConfig,
    hide: &[String],
    hide_urls: bool,
    pages: Option<&str>,
) -> anyhow::Result<()> {
    if !hide.is_empty() {
        if !config.obfuscation_patterns.is_empty() && config.hide.words.is_empty() {
            config.obfuscation_patterns.extend(hide.iter().cloned());
        } else {
            config.hide.words.extend(hide.iter().cloned());
        }
        config.enabled = true;
    }
    if hide_urls {
        config.hide.all_urls = true;
        config.enabled = true;
    }
    if let Some(spec) = pages {
        config.pages = parse_pages_cli(spec)?;
        config.page_stand_in = PageStandIn::Off;
        config.page_stand_ins.clear();
        config.enabled = true;
    }
    Ok(())
}

/// `--pages` value: `off`, `latin`, or a comma-separated word list.
pub fn parse_pages_cli(spec: &str) -> anyhow::Result<PagesSetting> {
    let trimmed = spec.trim();
    match trimmed {
        "off" => Ok(PagesSetting::Off),
        "latin" => Ok(PagesSetting::Latin),
        other => {
            let words: Vec<String> = other
                .split(',')
                .map(|word| word.trim().to_string())
                .filter(|word| !word.is_empty())
                .collect();
            if words.is_empty() {
                anyhow::bail!(
                    "provider filter --pages must be \"off\", \"latin\", or a word list, not {}",
                    quoted(other)
                );
            }
            Ok(PagesSetting::Words(words))
        }
    }
}

fn format_check_locked(
    config: &ProviderFilterConfig,
    text: Option<&str>,
) -> anyhow::Result<String> {
    let spec = normalize(config)?;
    let mut out = format_spec(&spec);
    let Some(text) = text else {
        return Ok(out);
    };
    let filter = ProviderFilter::compile(config)?;
    match filter.prepare_request(text.as_bytes()) {
        Ok(bytes) => {
            let sent = String::from_utf8_lossy(&bytes);
            out.push_str("would send: ");
            out.push_str(&sent);
            out.push('\n');
        }
        Err(ProviderError::FilterStopped { .. }) => {
            push_stop_line(&filter, &mut out);
        }
        Err(error) => anyhow::bail!("{error}"),
    }
    Ok(out)
}

const OUTBOUND_PANE_BODY_LIMIT: usize = 512 * 1024;
const OUTBOUND_PANE_TEXT_LIMIT: usize = 256 * 1024;

struct OutboundSlot {
    generation: u64,
    text: Arc<str>,
}

fn outbound_slot() -> &'static Mutex<OutboundSlot> {
    static SLOT: OnceLock<Mutex<OutboundSlot>> = OnceLock::new();
    SLOT.get_or_init(|| {
        Mutex::new(OutboundSlot {
            generation: 0,
            text: Arc::from(""),
        })
    })
}

/// Keep the latest provider-bound body for the side pane. The bytes are the
/// ones about to be written, after adjustments, page stand-in, and image
/// placeholders.
pub fn publish_outbound_body(body: &[u8]) {
    let text: Arc<str> = Arc::from(outbound_pane_text(body));
    if text.is_empty() {
        return;
    }
    let mut slot = outbound_slot()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let generation = slot.generation.wrapping_add(1).max(1);
    slot.generation = generation;
    slot.text = text;
}

/// Latest provider-bound body, if a request has been prepared.
pub fn outbound_view() -> Option<(u64, Arc<str>)> {
    let slot = outbound_slot()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    if slot.generation == 0 || slot.text.is_empty() {
        None
    } else {
        Some((slot.generation, Arc::clone(&slot.text)))
    }
}

/// Readable copy of a provider request. Fetched pages come first, as the
/// provider receives them, then the rest of the exchange, then a system prompt.
fn outbound_pane_text(body: &[u8]) -> String {
    let truncated = body.len() > OUTBOUND_PANE_BODY_LIMIT;
    let bytes = if truncated {
        &body[..OUTBOUND_PANE_BODY_LIMIT]
    } else {
        body
    };
    let mut text = match serde_json::from_slice::<serde_json::Value>(bytes) {
        Ok(value) => conversation_pane_text(&value)
            .unwrap_or_else(|| String::from_utf8_lossy(bytes).into_owned()),
        Err(_) => String::from_utf8_lossy(bytes).into_owned(),
    };
    if truncated {
        text.push_str("\n\n[truncated]");
    }
    if text.len() > OUTBOUND_PANE_TEXT_LIMIT {
        let mut end = OUTBOUND_PANE_TEXT_LIMIT;
        while !text.is_char_boundary(end) {
            end = end.saturating_sub(1);
        }
        text.truncate(end);
        text.push_str("\n\n[truncated]");
    }
    text
}

fn conversation_pane_text(value: &serde_json::Value) -> Option<String> {
    let object = value.as_object()?;
    if !object.contains_key("messages")
        && !object.contains_key("input")
        && !object.contains_key("system")
    {
        return None;
    }
    let mut ids = std::collections::BTreeSet::new();
    collect_fetch_ids(value, &mut ids);
    let mut pages = Vec::new();
    let mut exchange = Vec::new();
    let mut system = Vec::new();
    if let Some(system_value) = object.get("system") {
        push_plain(&mut system, plain_text(system_value));
    }
    if let Some(messages) = object.get("messages").and_then(|item| item.as_array()) {
        for message in messages {
            push_exchange_item(message, &ids, &mut pages, &mut exchange, &mut system);
        }
    }
    match object.get("input") {
        Some(serde_json::Value::Array(items)) => {
            for item in items {
                push_exchange_item(item, &ids, &mut pages, &mut exchange, &mut system);
            }
        }
        Some(input) => push_plain(&mut exchange, plain_text(input)),
        None => {}
    }
    if pages.is_empty() && exchange.is_empty() && system.is_empty() {
        return None;
    }
    let mut parts = pages;
    parts.extend(exchange);
    parts.extend(system);
    Some(parts.join("\n\n"))
}

fn push_plain(parts: &mut Vec<String>, text: String) {
    if !text.is_empty() {
        parts.push(text);
    }
}

fn push_exchange_item(
    value: &serde_json::Value,
    ids: &std::collections::BTreeSet<String>,
    pages: &mut Vec<String>,
    exchange: &mut Vec<String>,
    system: &mut Vec<String>,
) {
    let Some(map) = value.as_object() else {
        return;
    };
    if is_fetch_result(map, ids) {
        push_plain(pages, first_plain_text(map, &["content", "output"]));
        return;
    }
    let kind = map.get("type").and_then(|item| item.as_str());
    if matches!(kind, Some("function_call") | Some("tool_use")) {
        return;
    }
    let role = map.get("role").and_then(|item| item.as_str()).unwrap_or("");
    let text = block_text(map.get("content").or_else(|| map.get("output")), ids, pages);
    if role == "system" {
        push_plain(system, text);
    } else {
        push_plain(exchange, text);
    }
}

/// Message content, with `web_fetch` results pulled into `pages`.
fn block_text(
    value: Option<&serde_json::Value>,
    ids: &std::collections::BTreeSet<String>,
    pages: &mut Vec<String>,
) -> String {
    let Some(value) = value else {
        return String::new();
    };
    let serde_json::Value::Array(items) = value else {
        return plain_text(value);
    };
    let mut parts = Vec::new();
    for item in items {
        let Some(map) = item.as_object() else {
            push_plain(&mut parts, plain_text(item));
            continue;
        };
        let kind = map.get("type").and_then(|item| item.as_str());
        if kind == Some("tool_result") {
            let text = first_plain_text(map, &["content"]);
            let fetch = map
                .get("tool_use_id")
                .and_then(|item| item.as_str())
                .is_some_and(|id| ids.contains(id));
            if fetch {
                push_plain(pages, text);
            } else {
                push_plain(&mut parts, text);
            }
            continue;
        }
        if matches!(kind, Some("tool_use") | Some("image") | Some("image_url")) {
            continue;
        }
        if matches!(kind, Some("text") | Some("input_text")) {
            push_plain(&mut parts, first_plain_text(map, &["text"]));
        }
    }
    parts.join("\n")
}

fn first_plain_text(map: &serde_json::Map<String, serde_json::Value>, keys: &[&str]) -> String {
    for key in keys {
        if let Some(value) = map.get(*key) {
            let text = plain_text(value);
            if !text.is_empty() {
                return text;
            }
        }
    }
    String::new()
}

fn plain_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(items) => {
            let mut parts = Vec::new();
            for item in items {
                if let Some(text) = item.as_str() {
                    push_plain(&mut parts, text.to_string());
                    continue;
                }
                let Some(map) = item.as_object() else {
                    continue;
                };
                let kind = map.get("type").and_then(|item| item.as_str());
                if matches!(kind, Some("text") | Some("input_text")) {
                    if let Some(text) = map.get("text").and_then(|item| item.as_str()) {
                        push_plain(&mut parts, text.to_string());
                    }
                }
            }
            parts.join("\n")
        }
        _ => String::new(),
    }
}

pub fn tripped_rule() -> Option<String> {
    let filter = current();
    filter.is_tripped().then(|| {
        filter
            .reason
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
            .unwrap_or_else(|| "provider filter".to_string())
    })
}

/// Returns the trip reason once, so the session stop runs a single time.
pub fn claim_session_stop() -> Option<String> {
    let filter = current();
    if !filter.is_tripped() {
        return None;
    }
    if filter.session_stop_claimed.swap(true, Ordering::SeqCst) {
        return None;
    }
    tripped_rule()
}

/// Fetched pages keep their HTML source while a stand-in will rewrite it.
pub fn keeps_html_source() -> bool {
    current().keeps_html_source()
}

/// Outbound text is rewritten, so tool arguments may contain stand-ins.
pub fn rewrites_outbound() -> bool {
    let filter = current();
    filter.enabled && (!filter.adjustments.is_empty() || filter.page.is_some())
}

/// Replace stand-ins in tool arguments with the original file name or URL.
pub fn restore_local_value(value: &serde_json::Value) -> serde_json::Value {
    current().restore_value(value)
}

/// Put stand-ins back to the configured original in text the user reads.
///
/// Retain the original accumulated provider text and derive each displayed
/// copy from it once. Never feed a restored copy back into this function:
/// an original word may itself equal another replacement. Keeping the raw
/// accumulation also restores a stand-in split across chunks once complete.
/// The stop scan uses the raw provider bytes. A disabled filter, or one with
/// nothing to rewrite, leaves the text as it is.
pub fn restore_view_in_place(text: &mut String) {
    if text.is_empty() || !rewrites_outbound() {
        return;
    }
    let restored = current().restore_text(text);
    if restored != *text {
        *text = restored;
    }
}

pub fn scan_outbound(bytes: &[u8]) -> Result<(), ProviderError> {
    current().admit_outbound(bytes)
}

pub fn scan_outbound_chunk(carry: &mut Vec<u8>, chunk: &[u8]) -> Result<(), ProviderError> {
    current().admit_outbound_chunk(carry, chunk)
}

pub fn scan_fetched_page_chunk(carry: &mut Vec<u8>, chunk: &[u8]) -> Result<(), ProviderError> {
    current().admit_fetched_page_chunk(carry, chunk)
}

/// Status text for a withheld tool, file, or command result. It names the
/// rule id and does not include the matched bytes.
pub fn withheld_message(error: &ProviderError) -> String {
    let rule = match error {
        ProviderError::FilterStopped { rule } => rule.as_str(),
        _ => "provider filter",
    };
    format!(
        "Provider filter stopped this session ({rule}). The content was withheld and was not sent to the provider."
    )
}

pub fn session_stopped_status(rule: &str) -> String {
    let notice = current()
        .local_stop_notice()
        .unwrap_or_else(|| format!("Provider filter stopped this session ({rule})"));
    format!(
        "{notice}. Provider communication is cut off. Exit ca and start a new process to continue."
    )
}

fn should_replace(current: &ProviderFilter) -> bool {
    !current.enabled && !current.is_tripped()
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::{force_flag, notice_slot, process_slot, skip_install_flag, with_process_filter};
    use crate::agent::client::{AgentClient, ChatMessage, ChatRequestOptions};
    use crate::agent::stream::ProviderError;
    use crate::config::{PromptCacheMode, ProviderConfig, ProviderTimeoutConfig};
    use crate::request::{PreparedModelRequest, RequestKind};

    #[test]
    fn review_request_rewriting_preserves_protocol_and_json_strings() {
        let filter = adjusting(&[
            ("secret", "a\"b\nc"),
            ("model", "hidden-model"),
            ("user", "hidden-user"),
            ("text", "hidden-text"),
            ("read_file", "other-tool"),
        ]);
        let body = serde_json::json!({
            "model": "secret-model",
            "messages": [{
                "role": "user",
                "content": [{"type": "text", "text": "secret user text"}],
                "tool_calls": [{"id": "secret", "type": "function", "function": {
                    "name": "read_file", "arguments": "{\"path\":\"secret.txt\",\"user\":\"secret\"}"
                }}]
            }],
            "tools": [{"type":"function", "function": {
                "name": "read_file", "description": "secret text",
                "parameters": {"type":"object", "properties": {
                    "secret": {"type":"string", "description":"secret text"}
                }, "required":["secret"]}
            }}]
        });
        let output = filter
            .prepare_request(&serde_json::to_vec(&body).unwrap())
            .unwrap();
        let output: serde_json::Value =
            serde_json::from_slice(&output).expect("valid JSON after rewrite");
        assert_eq!(output["model"], "secret-model");
        assert_eq!(output["messages"][0]["role"], "user");
        assert_eq!(output["messages"][0]["content"][0]["type"], "text");
        assert_eq!(
            output["messages"][0]["content"][0]["text"],
            "a\"b\nc hidden-user hidden-text"
        );
        let call = &output["messages"][0]["tool_calls"][0];
        assert_eq!(call["id"], "secret");
        assert_eq!(call["function"]["name"], "read_file");
        let arguments: serde_json::Value =
            serde_json::from_str(call["function"]["arguments"].as_str().unwrap()).unwrap();
        assert_eq!(arguments["path"], "a\"b\nc.txt");
        assert_eq!(arguments["user"], "a\"b\nc");
        assert_eq!(output["tools"][0]["function"]["name"], "read_file");
        assert_eq!(
            output["tools"][0]["function"]["description"],
            "a\"b\nc hidden-text"
        );
        let schema = &output["tools"][0]["function"]["parameters"];
        assert_eq!(schema["properties"]["secret"]["type"], "string");
        assert_eq!(schema["required"][0], "secret");
        assert_eq!(
            schema["properties"]["secret"]["description"],
            "a\"b\nc hidden-text"
        );
    }

    #[test]
    fn review_custom_metadata_hides_text_and_preserves_provider_selectors() {
        let filter = adjusting(&[
            ("secret", "public"),
            ("high", "hidden-high"),
            ("low", "hidden-low"),
            ("auto", "hidden-auto"),
            ("default", "hidden-default"),
            ("reasoning", "hidden-reasoning"),
            ("explicit", "hidden-explicit"),
            ("text", "hidden-text"),
        ]);
        let schema = serde_json::json!({"type":"object","properties":{
            "secret":{"type":"string","description":"secret"}
        },"required":["secret"]});
        let body = serde_json::json!({
            "model":"secret-model", "messages":[{"role":"user","content":"secret"}],
            "metadata":{"patient":"secret","name":"secret"},
            "custom_context":{"customer_label":"secret"},
            "reasoning_effort":"high", "reasoning":{"effort":"high"},
            "output_config":{"effort":"low"}, "service_tier":"default",
            "include":["reasoning.encrypted_content"], "modalities":["text"],
            "truncation":"auto", "tool_choice":"auto", "prompt_cache_key":"secret-cache",
            "prompt_cache_options":{"mode":"explicit"},
            "input":[{"type":"reasoning","id":"secret-id","encrypted_content":"secret-state",
                "summary":[{"type":"summary_text","text":"secret"}]}],
            "text":{"verbosity":"low","format":{"type":"json_schema","name":"secret-format",
                "description":"secret","schema":schema.clone()}},
            "response_format":{"type":"json_schema","json_schema":{"name":"secret-format",
                "description":"secret","schema":schema}}
        });
        let output = filter
            .prepare_request(&serde_json::to_vec(&body).unwrap())
            .unwrap();
        let output: serde_json::Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(output["metadata"]["patient"], "public");
        assert_eq!(output["metadata"]["name"], "public");
        assert_eq!(output["custom_context"]["customer_label"], "public");
        assert_eq!(output["text"]["format"]["description"], "public");
        assert_eq!(output["text"]["format"]["name"], "secret-format");
        assert_eq!(
            output["text"]["format"]["schema"]["properties"]["secret"]["description"],
            "public"
        );
        assert_eq!(output["text"]["format"]["schema"]["required"][0], "secret");
        assert_eq!(
            output["response_format"]["json_schema"]["description"],
            "public"
        );
        assert_eq!(output["input"][0]["encrypted_content"], "secret-state");
        for key in [
            "model",
            "reasoning_effort",
            "reasoning",
            "output_config",
            "service_tier",
            "include",
            "modalities",
            "truncation",
            "tool_choice",
            "prompt_cache_key",
            "prompt_cache_options",
        ] {
            assert_eq!(output[key], body[key], "selector {key}");
        }
        assert_eq!(output["text"]["verbosity"], "low");
        let chosen = serde_json::json!({"messages":[],"tool_choice":{"type":"function","function":{"name":"secret"}}});
        let chosen_output = filter
            .prepare_request(&serde_json::to_vec(&chosen).unwrap())
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&chosen_output).unwrap(),
            chosen
        );
    }

    #[test]
    fn review_escaped_request_content_is_rewritten_and_scanned_decoded() {
        let filter = adjusting(&[("secret", "public")]);
        let output = filter
            .prepare_request(br#"{"messages":[{"role":"user","content":"s\u0065cret"}]}"#)
            .unwrap();
        let output: serde_json::Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(output["messages"][0]["content"], "public");
        let filter = enabled(vec![rule(
            "escaped",
            ProviderFilterTarget::Request,
            ProviderFilterKind::Literal,
            "a\nb",
        )]);
        assert!(filter
            .prepare_request(br#"{"messages":[{"role":"user","content":"a\nb"}]}"#)
            .is_err());
    }

    #[test]
    fn review_raw_json_files_and_json_inside_tool_content_are_content() {
        let filter = ProviderFilter::compile(&ProviderFilterConfig {
            enabled: true,
            obfuscate: true,
            phrases: vec!["secret".into()],
            ..ProviderFilterConfig::default()
        })
        .unwrap();
        let raw = br#"{"patient":"secret","model":"secret","name":"secret","id":"secret"}"#;
        assert!(filter.admit_outbound(raw).is_ok());
        let mut carry = Vec::new();
        assert!(filter.admit_outbound_chunk(&mut carry, raw).is_ok());
        let body = serde_json::json!({
            "model":"model-1", "messages":[{"role":"tool", "tool_call_id":"call-1",
            "content":std::str::from_utf8(raw).unwrap()}]
        });
        let output = filter
            .prepare_request(&serde_json::to_vec(&body).unwrap())
            .unwrap();
        let output: serde_json::Value = serde_json::from_slice(&output).unwrap();
        let content = output["messages"][0]["content"].as_str().unwrap();
        assert!(!content.contains("secret"), "{content}");
        assert_eq!(output["model"], "model-1");
        assert_eq!(output["messages"][0]["tool_call_id"], "call-1");
    }

    #[test]
    fn review_native_image_payloads_stay_opaque_during_text_rewrites() {
        let filter = adjusting(&[
            ("secret", "public"),
            ("image", "picture"),
            ("YWJj", "hidden"),
        ]);
        let image = serde_json::json!({"type":"image_url", "image_url":{"url":"data:image/png;base64,YWJj"}});
        let anthropic = serde_json::json!({"type":"image", "source":{"type":"base64","media_type":"image/png","data":"YWJj"}});
        let input_image =
            serde_json::json!({"type":"input_image", "image_url":"data:image/png;base64,YWJj"});
        let body = serde_json::json!({"model":"image-model", "messages":[{"role":"user", "content":[
            {"type":"text","text":"secret"}, image.clone(), anthropic.clone(), input_image.clone()
        ]}], "image":"data:image/png;base64,YWJj"});
        let output = filter
            .prepare_request(&serde_json::to_vec(&body).unwrap())
            .unwrap();
        let output: serde_json::Value = serde_json::from_slice(&output).unwrap();
        let content = &output["messages"][0]["content"];
        assert_eq!(content[0]["text"], "public");
        assert_eq!(content[1], image);
        assert_eq!(content[2], anthropic);
        assert_eq!(content[3], input_image);
        assert_eq!(output["image"], body["image"]);
    }

    #[test]
    fn review_new_hide_pairs_cannot_override_stop_entries() {
        for stop in [
            "[stop]\nphrases=[\"secret\"]",
            "[stop]\nurls=[\"https://secret.example\"]",
            "[[stop.rules]]\nid=\"explicit\"\npattern=\"secret\"",
        ] {
            let from = if stop.contains("urls") {
                "https://secret.example"
            } else {
                "secret"
            };
            let to = if stop.contains("urls") {
                "https://public.example"
            } else {
                "public"
            };
            let config: ProviderFilterConfig = toml::from_str(&format!(
                "enabled=true\n{stop}\n[[hide.pairs]]\nfrom={from:?}\nto={to:?}\n"
            ))
            .unwrap();
            let error = config
                .validate()
                .expect_err("new stop/hide conflict")
                .to_string();
            assert!(error.contains("stop and hide"), "{error}");
        }
    }

    #[test]
    fn review_unicode_urls_and_distinct_empty_path_segments() {
        for url in ["https://example.com/café", "http://例え.テスト"] {
            assert!(
                urls(&[url]).prepare_request(url.as_bytes()).is_err(),
                "missed {url}"
            );
            let filter = adjusting(&[(url, "http://public.example")]);
            assert_eq!(adjusted(&filter, url), "http://public.example");
        }
        assert!(urls(&["http://example.com/a/b"])
            .prepare_request(b"http://example.com/a//b")
            .is_ok());
        assert!(urls(&["http://example.com"])
            .prepare_request(b"http://example.com//")
            .is_ok());
        let filter = adjusting(&[("http://example.com//", "http://public.example")]);
        assert_eq!(
            adjusted(&filter, "http://example.com//"),
            "http://public.example"
        );
    }

    #[test]
    fn review_chunk_urls_wait_for_a_delimiter_or_explicit_eof() {
        for sample in [
            "http://stop.com.evil/path ",
            "http://stop.com/child ",
            "http://stop.com/é ",
            "http://stop.comé ",
        ] {
            for split in 1..sample.len() {
                let filter = urls(&["http://stop.com"]);
                let mut carry = Vec::new();
                assert!(
                    filter
                        .observe_response(&mut carry, &sample.as_bytes()[..split])
                        .is_ok(),
                    "first chunk {sample} at {split}"
                );
                assert!(
                    filter
                        .observe_response(&mut carry, &sample.as_bytes()[split..])
                        .is_ok(),
                    "second chunk {sample} at {split}"
                );
                assert!(filter.observe_response(&mut carry, b"").is_ok());
            }
        }
        let sample = b"http://stop.com";
        for split in 1..sample.len() {
            let filter = urls(&["http://stop.com"]);
            let mut carry = Vec::new();
            assert!(filter
                .observe_response(&mut carry, &sample[..split])
                .is_ok());
            assert!(filter
                .observe_response(&mut carry, &sample[split..])
                .is_ok());
            assert!(filter.observe_response(&mut carry, b"").is_err());
        }
        for suffix in ["?", "#"] {
            let filter = urls(&["http://stop.com"]);
            let mut carry = Vec::new();
            assert!(filter
                .observe_response(&mut carry, b"http://stop.com")
                .is_ok());
            let query = format!("{suffix}{}", "x".repeat(RESPONSE_CARRY_BYTES * 2));
            assert!(filter
                .observe_response(&mut carry, query.as_bytes())
                .is_err());
            assert!(carry.len() <= RESPONSE_CARRY_BYTES);
        }
        let filter = urls(&["http://stop.com"]);
        let mut carry = Vec::new();
        assert!(filter
            .admit_outbound_chunk(&mut carry, b"http://stop.com")
            .is_ok());
        assert!(filter.admit_outbound_chunk(&mut carry, b"/child").is_ok());
        assert!(filter.admit_outbound_chunk(&mut carry, b"").is_ok());
        let filter = urls(&["http://stop.com"]);
        assert!(filter
            .observe_response(&mut Vec::new(), br#"{"text":"http://stop.com"}"#)
            .is_err());
    }

    #[test]
    fn review_unicode_masks_use_the_same_matching_as_stops() {
        let config: ProviderFilterConfig =
            toml::from_str("enabled=true\n[[stop.phrases]]\ntext=\"İ\"\nmask=true").unwrap();
        let filter = ProviderFilter::compile(&config).unwrap();
        assert_eq!(adjusted(&filter, "i\u{307}"), "ca-kw-01");
    }

    #[test]
    fn review_ipv6_rewrite_has_one_pair_of_brackets() {
        let filter = adjusting(&[("http://example.com", "http://[::1]")]);
        assert_eq!(adjusted(&filter, "http://example.com/a"), "http://[::1]/a");
        let filter = adjusting(&[("http://[::1]", "http://public.example")]);
        assert_eq!(adjusted(&filter, "http://[::1]"), "http://public.example");
        assert!(urls(&["http://[::1]"])
            .prepare_request(b"[http://[::1]]")
            .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn review_trip_fifo_is_rejected_without_waiting_for_a_writer() {
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::OpenOptionsExt;

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("trip.fifo");
        let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
        let worker_path = path.clone();
        let (sender, receiver) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _ = sender.send(matches!(
                read_trip_file(&worker_path),
                TripRead::Unavailable
            ));
        });
        let result = receiver.recv_timeout(Duration::from_secs(1));
        if result.is_err() {
            // Release a regressed blocking open before failing this test.
            let _writer = std::fs::OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(path);
            let _ = receiver.recv_timeout(Duration::from_secs(1));
            panic!("trip-file open waited for a FIFO writer");
        }
        assert!(result.unwrap());
        worker.join().unwrap();
    }

    fn rule(
        id: &str,
        target: ProviderFilterTarget,
        kind: ProviderFilterKind,
        pattern: &str,
    ) -> ProviderFilterRule {
        ProviderFilterRule {
            id: id.into(),
            target,
            kind,
            pattern: pattern.into(),
        }
    }

    fn enabled(rules: Vec<ProviderFilterRule>) -> ProviderFilter {
        ProviderFilter::compile(&ProviderFilterConfig {
            enabled: true,
            trip_file: None,
            phrases: Vec::new(),
            urls: Vec::new(),
            rules,
            adjustments: Vec::new(),
            obfuscate: false,
            page_stand_in: PageStandIn::Off,
            page_stand_ins: Vec::new(),
            placeholder_images: false,
            obfuscation_patterns: Vec::new(),

            stop: FilterStop::default(),
            hide: FilterHide::default(),
            pages: PagesSetting::default(),
        })
        .expect("test filter compiles")
    }

    #[test]
    fn banner_is_on_only_while_the_filter_is_active() {
        let idle = ProviderFilter::compile(&ProviderFilterConfig::default()).expect("idle");
        assert!(!idle.shows_banner());
        let enabled_empty = ProviderFilter::compile(&ProviderFilterConfig {
            enabled: true,
            ..ProviderFilterConfig::default()
        })
        .expect("empty");
        assert!(enabled_empty.shows_banner());
        let armed = enabled(vec![rule(
            "needle",
            ProviderFilterTarget::Both,
            ProviderFilterKind::Literal,
            "NEEDLE",
        )]);
        assert!(armed.shows_banner());
        armed.trip("needle");
        assert!(armed.shows_banner());
        let adjusting = adjusting(&[("stop.txt", "go.txt")]);
        assert!(adjusting.shows_banner());
    }

    #[test]
    fn disabled_filter_does_not_match() {
        let filter = ProviderFilter::compile(&ProviderFilterConfig {
            enabled: false,
            trip_file: None,
            phrases: Vec::new(),
            urls: Vec::new(),
            rules: vec![rule(
                "needle",
                ProviderFilterTarget::Both,
                ProviderFilterKind::Literal,
                "NEEDLE",
            )],
            adjustments: Vec::new(),
            obfuscate: false,
            page_stand_in: PageStandIn::Off,
            page_stand_ins: Vec::new(),
            placeholder_images: false,
            obfuscation_patterns: Vec::new(),

            stop: FilterStop::default(),
            hide: FilterHide::default(),
            pages: PagesSetting::default(),
        })
        .expect("disabled filter compiles");
        assert!(filter.enforce_request(b"NEEDLE").is_ok());
        assert!(!filter.is_tripped());
    }

    #[test]
    fn literal_request_trip_is_sticky_and_hides_the_match() {
        let filter = enabled(vec![rule(
            "needle",
            ProviderFilterTarget::Request,
            ProviderFilterKind::Literal,
            "NEEDLE",
        )]);
        let error = filter.enforce_request(b"prefix NEEDLE suffix").unwrap_err();
        assert_eq!(
            error,
            ProviderError::FilterStopped {
                rule: "needle".into()
            }
        );
        assert!(!error.to_string().contains("NEEDLE"));
        let again = filter.enforce_request(b"clean").unwrap_err();
        assert_eq!(
            again,
            ProviderError::FilterStopped {
                rule: "needle".into()
            }
        );
        assert!(crate::fallback::cause_for_error(&again).is_none());
        assert!(!crate::turn_recovery::Category::of(&again).retryable());
    }

    #[test]
    fn a_word_matches_in_any_case_and_omitted_fields_mean_both_and_literal() {
        let config: ProviderFilterConfig =
            toml::from_str("enabled = true\n[[rules]]\nid = \"word\"\npattern = \"Secret\"\n")
                .expect("minimal rule parses");
        assert_eq!(config.rules[0].kind, ProviderFilterKind::Literal);
        assert_eq!(config.rules[0].target, ProviderFilterTarget::Both);
        let outbound = ProviderFilter::compile(&config).expect("filter compiles");
        let error = outbound.admit_outbound(b"see sEcReT here").unwrap_err();
        let rendered = error.to_string();
        assert!(!rendered.contains("Secret"));
        assert!(!rendered.contains("sEcReT"));
        let inbound = ProviderFilter::compile(&config).expect("filter compiles");
        assert!(inbound
            .observe_response(&mut Vec::new(), b"SECRET")
            .is_err());
    }

    #[test]
    fn a_quoted_phrase_matches_in_any_case_and_hides_the_text() {
        let config: ProviderFilterConfig =
            toml::from_str("enabled = true\nphrases = [\"A-123\", \"exact phrase\"]\n")
                .expect("phrases parse");
        assert!(config.rules.is_empty());
        let filter = ProviderFilter::compile(&config).expect("filter compiles");
        let error = filter
            .admit_outbound(b"ticket \"a-123\" listed")
            .unwrap_err();
        assert_eq!(
            error,
            ProviderError::FilterStopped {
                rule: "phrase-1".into()
            }
        );
        let rendered = error.to_string();
        assert!(!rendered.contains("A-123"));
        assert!(!rendered.contains("a-123"));

        let mut carry = Vec::new();
        let filter = ProviderFilter::compile(&config).expect("filter compiles");
        assert!(filter.admit_outbound_chunk(&mut carry, b"id A-12").is_ok());
        let error = filter
            .admit_outbound_chunk(&mut carry, b"3 end")
            .unwrap_err();
        assert_eq!(
            error,
            ProviderError::FilterStopped {
                rule: "phrase-1".into()
            }
        );

        let filter = ProviderFilter::compile(&config).expect("filter compiles");
        let error = filter
            .observe_response(&mut Vec::new(), b"An Exact Phrase.")
            .unwrap_err();
        assert_eq!(
            error,
            ProviderError::FilterStopped {
                rule: "phrase-2".into()
            }
        );
        assert!(!error.to_string().to_lowercase().contains("exact phrase"));

        let quotes_only: ProviderFilterConfig =
            toml::from_str("enabled = true\nphrases = [\"\\\"A-123\\\"\"]\n")
                .expect("quoted phrase parses");
        let filter = ProviderFilter::compile(&quotes_only).expect("filter compiles");
        assert!(filter.admit_outbound(b"bare a-123").is_ok());
        let error = filter.admit_outbound(b"see \"a-123\" now").unwrap_err();
        assert_eq!(
            error,
            ProviderError::FilterStopped {
                rule: "phrase-1".into()
            }
        );
        assert!(!error.to_string().contains("A-123"));

        let named: ProviderFilterConfig =
            toml::from_str("enabled = true\n[[rules]]\nid = \"ticket\"\npattern = \"A-123\"\n")
                .expect("named phrase parses");
        assert_eq!(named.rules[0].kind, ProviderFilterKind::Literal);
        let filter = ProviderFilter::compile(&named).expect("filter compiles");
        let error = filter.admit_outbound(b"see a-123 now").unwrap_err();
        assert_eq!(
            error,
            ProviderError::FilterStopped {
                rule: "ticket".into()
            }
        );
        assert!(!error.to_string().contains("A-123"));
        assert!(!error.to_string().contains("a-123"));

        let disabled: ProviderFilterConfig =
            toml::from_str("enabled = false\nphrases = [\"A-123\"]\n").expect("disabled parses");
        let filter = ProviderFilter::compile(&disabled).expect("filter compiles");
        assert!(filter.admit_outbound(b"\"A-123\"").is_ok());
        assert!(!filter.is_tripped());
    }

    #[test]
    fn blank_phrase_and_phrase_id_collision_fail_without_echoing_the_phrase() {
        let blank: ProviderFilterConfig = toml::from_str("phrases = [\"\"]\n").expect("parses");
        let error = blank.validate().unwrap_err().to_string();
        assert!(error.contains("phrase 1"));
        assert!(!error.contains("A-123"));
        let spaces: ProviderFilterConfig = toml::from_str("phrases = [\"   \"]\n").expect("parses");
        assert!(spaces.validate().is_err());
        let collision: ProviderFilterConfig = toml::from_str(
            "phrases = [\"A-123\"]\n[[rules]]\nid = \"phrase-1\"\npattern = \"x\"\n",
        )
        .expect("parses");
        let error = collision.validate().unwrap_err().to_string();
        assert!(error.contains("phrase-1"));
        assert!(!error.contains("A-123"));
    }

    fn urls(entries: &[&str]) -> ProviderFilter {
        ProviderFilter::compile(&ProviderFilterConfig {
            enabled: true,
            trip_file: None,
            phrases: Vec::new(),
            urls: entries.iter().map(|entry| (*entry).to_string()).collect(),
            rules: Vec::new(),
            adjustments: Vec::new(),
            obfuscate: false,
            page_stand_in: PageStandIn::Off,
            page_stand_ins: Vec::new(),
            placeholder_images: false,
            obfuscation_patterns: Vec::new(),

            stop: FilterStop::default(),
            hide: FilterHide::default(),
            pages: PagesSetting::default(),
        })
        .expect("url filter compiles")
    }

    #[test]
    fn configured_url_matches_that_url_and_not_a_different_one() {
        let origin = urls(&["http://stop.com"]);
        let error = origin
            .admit_outbound(b"see http://stop.com now")
            .unwrap_err();
        assert_eq!(
            error,
            ProviderError::FilterStopped {
                rule: "url-1".into()
            }
        );
        let rendered = error.to_string();
        assert!(!rendered.contains("stop.com"));
        assert!(!rendered.contains("http://"));

        let origin = urls(&["http://stop.com"]);
        assert!(origin.admit_outbound(b"see HTTP://Stop.com/ now").is_err());
        let origin = urls(&["http://stop.com"]);
        assert!(origin.admit_outbound(b"see \"http://stop.com\".").is_err());
        let origin = urls(&["http://stop.com"]);
        assert!(origin
            .admit_outbound(b"see http://stop.com:80?utm=1")
            .is_err());

        let origin = urls(&["http://stop.com"]);
        for sample in [
            b"see http://anything.else".as_slice(),
            b"see http://stop.com/stop",
            b"see http://stop.com.evil",
            b"see http://notstop.com",
            b"see http://sub.stop.com",
            b"see https://stop.com",
            b"see http://stop.com:8080",
            b"see http://stop.com@evil.com",
            b"see http://stop.com/stop/more",
        ] {
            assert!(
                origin.admit_outbound(sample).is_ok(),
                "origin matched {}",
                String::from_utf8_lossy(sample)
            );
        }

        let path = urls(&["http://stop.com/stop"]);
        let error = path
            .admit_outbound(b"open \"http://stop.com/stop\" please")
            .unwrap_err();
        assert_eq!(
            error,
            ProviderError::FilterStopped {
                rule: "url-1".into()
            }
        );
        assert!(!error.to_string().contains("stop.com"));
        assert!(urls(&["http://stop.com/stop"])
            .admit_outbound(b"http://stop.com/stop/")
            .is_err());
        assert!(urls(&["http://stop.com/stop"])
            .admit_outbound(b"http://stop.com/stop?x=1")
            .is_err());
        assert!(urls(&["http://stop.com/stop"])
            .admit_outbound(b"(http://stop.com/stop)")
            .is_err());
        let path = urls(&["http://stop.com/stop"]);
        let mut reply = Vec::new();
        assert!(path
            .observe_response(&mut reply, b"reply http://Stop.com/stop.")
            .is_ok());
        assert!(path.observe_response(&mut reply, b"").is_err());

        let path = urls(&["http://stop.com/stop"]);
        for sample in [
            b"http://stop.com".as_slice(),
            b"http://anything.else",
            b"http://anything.else/stop",
            b"http://stop.com/stopped",
            b"http://stop.com/stop/more",
            b"http://stop.com.evil/stop",
            b"https://stop.com/stop",
            b"http://notstop.com/stop",
        ] {
            assert!(
                path.admit_outbound(sample).is_ok(),
                "path matched {}",
                String::from_utf8_lossy(sample)
            );
        }

        let mut carry = Vec::new();
        let path = urls(&["http://stop.com/stop"]);
        assert!(path
            .admit_outbound_chunk(&mut carry, b"id http://stop.c")
            .is_ok());
        let error = path
            .admit_outbound_chunk(&mut carry, b"om/stop end")
            .unwrap_err();
        assert_eq!(
            error,
            ProviderError::FilterStopped {
                rule: "url-1".into()
            }
        );

        let both = urls(&["http://stop.com", "http://stop.com/stop"]);
        assert!(both.admit_outbound(b"only http://anything.else").is_ok());
        let error = both
            .admit_outbound(b"then http://stop.com/stop")
            .unwrap_err();
        assert_eq!(
            error,
            ProviderError::FilterStopped {
                rule: "url-2".into()
            }
        );

        let disabled: ProviderFilterConfig =
            toml::from_str("enabled = false\nurls = [\"http://stop.com\"]\n")
                .expect("disabled url config parses");
        let filter = ProviderFilter::compile(&disabled).expect("disabled url filter");
        assert!(filter.admit_outbound(b"http://stop.com").is_ok());
        assert!(!filter.is_tripped());
    }

    #[test]
    fn invalid_url_fails_without_echoing_the_url() {
        let relative: ProviderFilterConfig =
            toml::from_str("urls = [\"stop.com\"]\n").expect("parses");
        let error = relative.validate().unwrap_err().to_string();
        assert!(error.contains("url 1"));
        assert!(error.contains("\"stop.com\""));
        let blank: ProviderFilterConfig = toml::from_str("urls = [\"\"]\n").expect("parses");
        assert!(blank.validate().is_err());
        let other_scheme: ProviderFilterConfig =
            toml::from_str("urls = [\"ftp://stop.com/secret\"]\n").expect("parses");
        let error = other_scheme.validate().unwrap_err().to_string();
        assert!(error.contains("url 1"));
        assert!(error.contains("\"ftp://stop.com/secret\""));
        let collision: ProviderFilterConfig = toml::from_str(
            "urls = [\"http://stop.com\"]\n[[rules]]\nid = \"url-1\"\npattern = \"x\"\n",
        )
        .expect("parses");
        let error = collision.validate().unwrap_err().to_string();
        assert!(error.contains("url-1"));
        assert!(!error.contains("stop.com"));
    }

    #[test]
    fn outbound_content_trip_spans_reads_and_hides_the_match() {
        let filter = enabled(vec![rule(
            "needle",
            ProviderFilterTarget::Request,
            ProviderFilterKind::Literal,
            "NEEDLE",
        )]);
        let mut carry = Vec::new();
        assert!(filter.admit_outbound_chunk(&mut carry, b"file NEE").is_ok());
        let error = filter
            .admit_outbound_chunk(&mut carry, b"DLE more")
            .unwrap_err();
        let message = withheld_message(&error);
        assert!(!message.contains("NEEDLE"));
        assert!(message.contains("needle"));
        assert!(filter.admit_outbound(b"clean").is_err());
    }

    #[test]
    fn response_rule_ignores_outbound_content() {
        let filter = enabled(vec![rule(
            "phrase",
            ProviderFilterTarget::Response,
            ProviderFilterKind::Literal,
            "NEEDLE",
        )]);
        assert!(filter.admit_outbound(b"file NEEDLE").is_ok());
        assert!(!filter.is_tripped());
    }

    #[test]
    fn response_rule_ignores_the_request_and_spans_chunks() {
        let filter = enabled(vec![rule(
            "phrase",
            ProviderFilterTarget::Response,
            ProviderFilterKind::Literal,
            "STOP-NOW",
        )]);
        assert!(filter.enforce_request(b"STOP-NOW").is_ok());
        let mut carry = Vec::new();
        assert!(filter.observe_response(&mut carry, b"hello STO").is_ok());
        let error = filter
            .observe_response(&mut carry, b"P-NOW tail")
            .unwrap_err();
        assert_eq!(
            error,
            ProviderError::FilterStopped {
                rule: "phrase".into()
            }
        );
    }

    #[test]
    fn regex_matches_request_bytes() {
        let filter = enabled(vec![rule(
            "token",
            ProviderFilterTarget::Both,
            ProviderFilterKind::Regex,
            r"sk-[A-Za-z0-9]{4}",
        )]);
        let error = filter.enforce_request(b"key=sk-abcd").unwrap_err();
        assert_eq!(
            error,
            ProviderError::FilterStopped {
                rule: "token".into()
            }
        );
    }

    #[test]
    fn invalid_regex_and_relative_trip_file_fail_closed() {
        let bad_regex = ProviderFilterConfig {
            enabled: true,
            trip_file: None,
            phrases: Vec::new(),
            urls: Vec::new(),
            rules: vec![rule(
                "bad",
                ProviderFilterTarget::Request,
                ProviderFilterKind::Regex,
                "(",
            )],
            adjustments: Vec::new(),
            obfuscate: false,
            page_stand_in: PageStandIn::Off,
            page_stand_ins: Vec::new(),
            placeholder_images: false,
            obfuscation_patterns: Vec::new(),

            stop: FilterStop::default(),
            hide: FilterHide::default(),
            pages: PagesSetting::default(),
        };
        assert!(bad_regex.validate().is_err());
        let relative = ProviderFilterConfig {
            enabled: true,
            trip_file: Some(PathBuf::from("stop")),
            phrases: Vec::new(),
            urls: Vec::new(),
            rules: Vec::new(),
            adjustments: Vec::new(),
            obfuscate: false,
            page_stand_in: PageStandIn::Off,
            page_stand_ins: Vec::new(),
            placeholder_images: false,
            obfuscation_patterns: Vec::new(),

            stop: FilterStop::default(),
            hide: FilterHide::default(),
            pages: PagesSetting::default(),
        };
        assert!(relative.validate().is_err());
    }

    #[cfg(unix)]
    #[test]
    fn trip_file_requires_exact_stop_and_rejects_a_symlink() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("trip");
        let filter = ProviderFilter::compile(&ProviderFilterConfig {
            enabled: true,
            trip_file: Some(path.clone()),
            phrases: Vec::new(),
            urls: Vec::new(),
            rules: Vec::new(),
            adjustments: Vec::new(),
            obfuscate: false,
            page_stand_in: PageStandIn::Off,
            page_stand_ins: Vec::new(),
            placeholder_images: false,
            obfuscation_patterns: Vec::new(),

            stop: FilterStop::default(),
            hide: FilterHide::default(),
            pages: PagesSetting::default(),
        })
        .expect("trip filter");
        assert!(filter.enforce_request(b"hello").is_ok());
        std::fs::write(&path, "continue\n").expect("write");
        filter
            .last_trip_poll
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .take();
        assert!(filter.enforce_request(b"hello").is_ok());
        std::fs::write(&path, " stop \n").expect("write stop");
        filter
            .last_trip_poll
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .take();
        let error = filter.enforce_request(b"hello").unwrap_err();
        assert_eq!(
            error,
            ProviderError::FilterStopped {
                rule: "trip file".into()
            }
        );

        let link_path = directory.path().join("link");
        std::os::unix::fs::symlink(&path, &link_path).expect("symlink");
        let linked = ProviderFilter::compile(&ProviderFilterConfig {
            enabled: true,
            trip_file: Some(link_path),
            phrases: Vec::new(),
            urls: Vec::new(),
            rules: Vec::new(),
            adjustments: Vec::new(),
            obfuscate: false,
            page_stand_in: PageStandIn::Off,
            page_stand_ins: Vec::new(),
            placeholder_images: false,
            obfuscation_patterns: Vec::new(),

            stop: FilterStop::default(),
            hide: FilterHide::default(),
            pages: PagesSetting::default(),
        })
        .expect("link filter");
        let error = linked.enforce_request(b"hello").unwrap_err();
        assert_eq!(
            error,
            ProviderError::FilterStopped {
                rule: "trip file unavailable".into()
            }
        );
    }

    #[test]
    fn enabled_install_cannot_be_disarmed() {
        let disabled = ProviderFilter::compile(&ProviderFilterConfig::default()).expect("disabled");
        assert!(super::should_replace(&disabled));
        let enabled = enabled(vec![rule(
            "keep",
            ProviderFilterTarget::Both,
            ProviderFilterKind::Literal,
            "KEEP",
        )]);
        assert!(!super::should_replace(&enabled));
        enabled.trip("keep");
        assert!(!super::should_replace(&enabled));
    }

    #[tokio::test]
    async fn client_refuses_a_matching_request_without_a_transport_error() {
        let filter = Arc::new(enabled(vec![rule(
            "needle",
            ProviderFilterTarget::Request,
            ProviderFilterKind::Literal,
            "NEEDLE",
        )]));
        let provider = ProviderConfig {
            google: None,
            tool_schema_policy: Default::default(),
            base_url: "http://127.0.0.1:9/v1".into(),
            api_key: String::new(),
            protocol: crate::agent::ProviderProtocol::default(),
            prompt_cache_mode: PromptCacheMode::Implicit,
            models: vec!["model".into()],
            timeout: ProviderTimeoutConfig::default(),
        };
        let policy = crate::agent::client::ProviderCallPolicy {
            connect_timeout: Duration::from_millis(200),
            headers_timeout: Duration::from_millis(200),
            first_event_timeout: Duration::from_millis(200),
            no_progress_timeout: Duration::from_millis(200),
            max_attempts: 1,
            ..Default::default()
        };
        let client = AgentClient::with_call_policy(
            "example",
            &provider,
            "model",
            policy,
            crate::request::RequestStore::default(),
        )
        .with_filter_for_test(filter);
        let prepared = PreparedModelRequest::new(
            RequestKind::Compatibility,
            "example",
            "model",
            vec![ChatMessage {
                role: "user".into(),
                content: crate::image_content::ModelContent::text("please NEEDLE"),
                tool_calls: None,
                tool_call_id: None,
                response_items: Vec::new(),
            }],
            Vec::new(),
            ChatRequestOptions::default(),
        );
        let error = client.stream_prepared(&prepared).await.unwrap_err();
        assert_eq!(
            error,
            ProviderError::FilterStopped {
                rule: "needle".into()
            }
        );
        assert!(!matches!(error, ProviderError::Transport(_)));
    }

    fn adjusting(pairs: &[(&str, &str)]) -> ProviderFilter {
        ProviderFilter::compile(&ProviderFilterConfig {
            enabled: true,
            adjustments: pairs
                .iter()
                .map(|(from, to)| ContentAdjustment {
                    from: (*from).to_string(),
                    to: (*to).to_string(),
                })
                .collect(),
            ..ProviderFilterConfig::default()
        })
        .expect("adjustments compile")
    }

    fn adjusted(filter: &ProviderFilter, text: &str) -> String {
        let bytes = filter
            .prepare_request(text.as_bytes())
            .expect("adjustment does not trip");
        String::from_utf8(bytes).expect("adjusted text")
    }

    #[test]
    fn adjustments_rewrite_file_and_page_text_before_the_provider_sees_it() {
        let filter = adjusting(&[("stop.txt", "go.txt"), ("http://stop.com", "http://go.com")]);
        let source = "\
file stop.txt says the path is stop.txt.bak and Stop.txt changes.
page says see \"http://stop.com\". and http://stop.com/docs?q=1#top.
also (http://STOP.com/help) and HTTP://stop.com/more.
leave http://anything.else/docs, http://stop.com.evil/docs, \
https://stop.com/docs, http://notstop.com/docs, http://stop.com:8080/docs, \
and http://stop.com@evil.com/docs.
";
        let rewritten = adjusted(&filter, source);
        assert_eq!(
            rewritten,
            "\
file go.txt says the path is go.txt.bak and go.txt changes.
page says see \"http://go.com\". and http://go.com/docs?q=1#top.
also (http://go.com/help) and http://go.com/more.
leave http://anything.else/docs, http://stop.com.evil/docs, \
https://stop.com/docs, http://notstop.com/docs, http://stop.com:8080/docs, \
and http://stop.com@evil.com/docs.
"
        );
        assert_eq!(
            adjusted(
                &adjusting(&[("http://stop.com", "http://go.com")]),
                "http://anything.else/stop.txt"
            ),
            "http://anything.else/stop.txt"
        );
        assert_eq!(
            adjusted(&filter, "http://anything.else/stop.txt"),
            "http://anything.else/go.txt"
        );

        let path = adjusting(&[("http://stop.com/stop", "http://go.com")]);
        assert_eq!(
            adjusted(&path, "open http://stop.com/stop/more?q=1#f."),
            "open http://go.com/more?q=1#f."
        );
        assert_eq!(adjusted(&path, "http://stop.com/stop"), "http://go.com");
        assert_eq!(adjusted(&path, "http://stop.com/stop/"), "http://go.com/");
        for sample in [
            "http://stop.com",
            "http://stop.com/stopped",
            "http://stop.com/docs",
            "http://anything.else/stop",
        ] {
            assert_eq!(adjusted(&path, sample), sample, "{sample}");
        }

        let moved = adjusting(&[("http://stop.com", "https://go.com:8443/go")]);
        assert_eq!(
            adjusted(&moved, "http://stop.com/docs"),
            "https://go.com:8443/go/docs"
        );
        let once = adjusting(&[("http://stop.com", "http://stop.com/out")]);
        assert_eq!(
            adjusted(&once, "http://stop.com/docs"),
            "http://stop.com/out/docs"
        );
        assert_eq!(adjusted(&adjusting(&[("a", "aa")]), "a"), "aa");
        assert_eq!(adjusted(&adjusting(&[("a", "b"), ("b", "c")]), "a"), "c");
        let detected = adjusting(&[("secret", "redacted"), ("stop.txt", "go.txt")]);
        let rewritten = adjusted(
            &detected,
            r#"{"content":"Secret STOP.TXT and secret stop.txt"}"#,
        );
        let folded = rewritten.to_ascii_lowercase();
        assert!(!folded.contains("secret"), "{rewritten}");
        assert!(!folded.contains("stop.txt"), "{rewritten}");
        assert_eq!(
            rewritten,
            r#"{"content":"redacted go.txt and redacted go.txt"}"#
        );

        let mut window = b"stop.txt".to_vec();
        window.insert(0, 0x80);
        window.push(0xC2);
        assert_eq!(
            filter.prepare_request(&window).expect("edge text"),
            b"\x80go.txt\xC2"
        );
        let mut binary = b"stop.txt".to_vec();
        binary.push(0xff);
        binary.extend_from_slice(b"stop.txt");
        assert_eq!(
            filter.prepare_request(&binary).expect("binary stays"),
            binary
        );
    }

    #[test]
    fn adjustments_change_what_a_request_stop_can_see() {
        let removed: ProviderFilterConfig = toml::from_str(
            "\
enabled = true
phrases = [\"stop.txt\"]
[[adjustments]]
from = \"stop.txt\"
to = \"go.txt\"
",
        )
        .expect("parses");
        let filter = ProviderFilter::compile(&removed).expect("compiles");
        assert_eq!(
            adjusted(&filter, "the file says stop.txt"),
            "the file says go.txt"
        );
        assert_eq!(
            adjusted(&filter, "the file says STOP.TXT"),
            "the file says go.txt"
        );
        assert!(filter.admit_outbound(b"the file says Stop.txt").is_ok());
        assert!(!filter.is_tripped());
        let mut carry = Vec::new();
        assert!(filter.admit_outbound_chunk(&mut carry, b"stop.tx").is_ok());
        assert!(filter.admit_outbound_chunk(&mut carry, b"t").is_ok());
        assert!(!filter.is_tripped());
        let response = ProviderFilter::compile(&removed).expect("compiles");
        let error = response
            .observe_response(&mut Vec::new(), b"reply stop.txt")
            .unwrap_err();
        assert_eq!(
            error,
            ProviderError::FilterStopped {
                rule: "phrase-1".into()
            }
        );
        assert!(!error.to_string().contains("stop.txt"));

        let introduced: ProviderFilterConfig = toml::from_str(
            "\
enabled = true
phrases = [\"go.txt\"]
[[adjustments]]
from = \"stop.txt\"
to = \"go.txt\"
",
        )
        .expect("parses");
        let filter = ProviderFilter::compile(&introduced).expect("compiles");
        let error = filter.prepare_request(b"page text stop.txt").unwrap_err();
        assert_eq!(
            error,
            ProviderError::FilterStopped {
                rule: "phrase-1".into()
            }
        );
        assert!(!error.to_string().contains("go.txt"));
        assert!(!error.to_string().contains("stop.txt"));
        let response = ProviderFilter::compile(&introduced).expect("compiles");
        assert!(response
            .observe_response(&mut Vec::new(), b"page text stop.txt")
            .is_ok());
        let mut carry = Vec::new();
        let filter = ProviderFilter::compile(&introduced).expect("compiles");
        assert!(filter.admit_outbound_chunk(&mut carry, b"stop.tx").is_ok());
        let error = filter
            .admit_outbound_chunk(&mut carry, b"t inside")
            .unwrap_err();
        assert_eq!(
            error,
            ProviderError::FilterStopped {
                rule: "phrase-1".into()
            }
        );

        let url_stop: ProviderFilterConfig = toml::from_str(
            "\
enabled = true
urls = [\"http://go.com/docs\"]
[[adjustments]]
from = \"http://stop.com\"
to = \"http://go.com\"
",
        )
        .expect("parses");
        let filter = ProviderFilter::compile(&url_stop).expect("compiles");
        assert!(filter.admit_outbound(b"fetched http://stop.com").is_ok());
        let error = filter
            .admit_outbound(b"fetched http://stop.com/docs")
            .unwrap_err();
        assert_eq!(
            error,
            ProviderError::FilterStopped {
                rule: "url-1".into()
            }
        );
        assert!(!error.to_string().contains("go.com"));
        assert!(!error.to_string().contains("stop.com"));
        let response = ProviderFilter::compile(&url_stop).expect("compiles");
        assert!(response
            .observe_response(&mut Vec::new(), b"fetched http://stop.com/docs")
            .is_ok());
    }

    #[test]
    fn disabled_or_invalid_adjustments_do_not_rewrite_or_echo() {
        let disabled: ProviderFilterConfig = toml::from_str(
            "\
enabled = false
[[adjustments]]
from = \"stop.txt\"
to = \"go.txt\"
",
        )
        .expect("parses");
        let filter = ProviderFilter::compile(&disabled).expect("compiles");
        assert!(!filter.shows_banner());
        assert_eq!(
            filter.prepare_request(b"stop.txt").expect("disabled"),
            b"stop.txt"
        );

        let bad_url: ProviderFilterConfig = toml::from_str(
            "\
[[adjustments]]
from = \"http://stop.com/secret\"
to = \"plain\"
",
        )
        .expect("parses");
        let error = bad_url.validate().unwrap_err().to_string();
        assert!(error.contains("adjustment 1"));
        assert!(error.contains("\"http://stop.com/secret\""));
        assert!(error.contains("\"plain\""));

        let blank: ProviderFilterConfig =
            toml::from_str("[[adjustments]]\nfrom = \"   \"\nto = \"go\"\n").expect("parses");
        let error = blank.validate().unwrap_err().to_string();
        assert!(error.contains("adjustment 1"));
        assert!(!error.contains("go"));

        let mut too_many = ProviderFilterConfig::default();
        for index in 0..33 {
            too_many.adjustments.push(ContentAdjustment {
                from: format!("k{index}"),
                to: "v".into(),
            });
        }
        let error = too_many.validate().unwrap_err().to_string();
        assert!(error.contains("32"));
        assert!(!error.contains("k32"));
    }

    #[test]
    fn obfuscation_switch_rewrites_a_local_page_and_file_without_touching_disk() {
        let directory = tempfile::tempdir().expect("tempdir");
        let file_name = "stop.txt";
        let path = directory.path().join(file_name);
        let original = "secret plans in stop.txt\n";
        std::fs::write(&path, original).expect("write");

        let page = "<html><body><a href=\"http://stop.example/hidden\">secret</a> see stop.txt</body></html>";
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let address = listener.local_addr().expect("addr");
        let served = page.to_string();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut ignored = [0_u8; 2048];
            let _ = std::io::Read::read(&mut stream, &mut ignored);
            let body = served.as_bytes();
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            std::io::Write::write_all(&mut stream, header.as_bytes()).expect("header");
            std::io::Write::write_all(&mut stream, body).expect("body");
        });
        let mut stream = std::net::TcpStream::connect(address).expect("connect");
        std::io::Write::write_all(
            &mut stream,
            b"GET /page HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        )
        .expect("request");
        let mut received = Vec::new();
        std::io::Read::read_to_end(&mut stream, &mut received).expect("response");
        server.join().expect("server");
        let received = String::from_utf8(received).expect("utf8");
        let source = received.split("\r\n\r\n").nth(1).expect("html source");
        assert_eq!(source, page);

        let filter = ProviderFilter::compile(&ProviderFilterConfig {
            enabled: true,
            obfuscate: true,
            phrases: vec!["secret".into(), "stop.txt".into()],
            urls: vec!["http://stop.example/hidden".into()],
            ..ProviderFilterConfig::default()
        })
        .expect("compiles");
        assert!(filter.keeps_html_source());

        let outbound_page = adjusted(&filter, source);
        assert!(outbound_page.contains("<html>"), "{outbound_page}");
        assert!(
            outbound_page.contains("<a href=\"http://ca-r01.invalid\">"),
            "{outbound_page}"
        );
        assert!(outbound_page.contains(">ca-kw-01<"), "{outbound_page}");
        assert!(outbound_page.contains("see ca-kw-02"), "{outbound_page}");
        let folded = outbound_page.to_ascii_lowercase();
        assert!(!folded.contains("secret"), "{outbound_page}");
        assert!(!outbound_page.contains("stop.txt"), "{outbound_page}");
        assert!(!outbound_page.contains("stop.example"), "{outbound_page}");

        assert_eq!(adjusted(&filter, original), "ca-kw-01 plans in ca-kw-02\n");
        assert_eq!(adjusted(&filter, file_name), "ca-kw-02");
        assert_eq!(std::fs::read_to_string(&path).expect("disk"), original);

        let restored_name = filter.restore_text(file_name);
        assert_eq!(restored_name, file_name);
        let restored_tool_name = filter.restore_text("ca-kw-02");
        assert_eq!(restored_tool_name, "stop.txt");
        let restored_path = directory.path().join(&restored_tool_name);
        assert_eq!(
            std::fs::read_to_string(&restored_path).expect("opened real file"),
            original
        );

        let child = adjusted(&filter, "http://stop.example/hidden/more?q=1");
        assert_eq!(child, "http://ca-r01.invalid/more?q=1");
        assert_eq!(
            filter.restore_text(&child),
            "http://stop.example/hidden/more?q=1"
        );
        assert_eq!(
            adjusted(&filter, "http://stop.example/other"),
            "http://stop.example/other"
        );
        assert_eq!(
            adjusted(&filter, "http://anything.else/hidden"),
            "http://anything.else/hidden"
        );

        let stopped = ProviderFilter::compile(&ProviderFilterConfig {
            enabled: true,
            obfuscate: false,
            phrases: vec!["secret".into()],
            ..ProviderFilterConfig::default()
        })
        .expect("compiles");
        assert!(!stopped.keeps_html_source());
        let error = stopped.prepare_request(b"secret").unwrap_err();
        assert!(matches!(error, ProviderError::FilterStopped { .. }));

        let chosen = ProviderFilter::compile(&ProviderFilterConfig {
            enabled: true,
            obfuscate: true,
            phrases: vec!["stop.txt".into()],
            adjustments: vec![ContentAdjustment {
                from: "stop.txt".into(),
                to: "go.txt".into(),
            }],
            ..ProviderFilterConfig::default()
        })
        .expect("compiles");
        assert_eq!(adjusted(&chosen, "Stop.txt"), "go.txt");
    }

    #[test]
    fn obfuscation_switch_rewrites_redactme_tokens_in_html_text_and_js() {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/provider-filter");
        let filter = ProviderFilter::compile(&ProviderFilterConfig {
            enabled: true,
            obfuscate: true,
            phrases: vec!["redactme".into()],
            ..ProviderFilterConfig::default()
        })
        .expect("compiles");

        for name in ["redactme.html", "redactme.txt", "redactme.js"] {
            let path = root.join(name);
            let original = std::fs::read_to_string(&path).expect("fixture");
            assert!(
                original.matches("redactme").count() >= 3,
                "{name} needs several lowercase tokens"
            );
            assert!(
                original.matches("REDACTME").count() >= 2,
                "{name} needs several uppercase tokens"
            );
            let outbound = adjusted(&filter, &original);
            assert_eq!(
                count_ascii_keyword(&original, "redactme"),
                count_ascii_keyword(&outbound, "ca-kw-01"),
                "{name} outbound:\n{outbound}"
            );
            assert_eq!(count_ascii_keyword(&outbound, "redactme"), 0, "{outbound}");
            assert!(
                !outbound.to_ascii_lowercase().contains("redactme"),
                "{outbound}"
            );
            assert_eq!(std::fs::read_to_string(&path).expect("disk"), original);
            let restored = filter.restore_text(&outbound);
            assert_eq!(
                count_ascii_keyword(&restored, "redactme"),
                count_ascii_keyword(&original, "redactme")
            );
            assert!(!restored.contains("REDACTME"));
            assert!(!restored.contains("ca-kw-01"));
            let extension = name.rsplit_once('.').expect("extension").1;
            let stand_in_name = format!("ca-kw-01.{extension}");
            assert_eq!(adjusted(&filter, name), stand_in_name);
            assert_eq!(
                filter.restore_text(&stand_in_name),
                format!("redactme.{extension}")
            );
        }

        let notes = std::fs::read_to_string(root.join("redactme.txt")).expect("notes");
        let outbound_notes = adjusted(&filter, &notes);
        assert!(
            outbound_notes.contains("Leave redacted alone."),
            "{outbound_notes}"
        );
        assert!(outbound_notes.contains("ca-kw-01 is the first token."));
        assert!(outbound_notes.contains("ca-kw-01 is the second token."));
        assert!(outbound_notes.contains("ca-kw-01.txt"));
    }

    #[test]
    fn explicit_adjustment_rewrites_redactme_tokens_to_redacted() {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/provider-filter");
        let filter = ProviderFilter::compile(&ProviderFilterConfig {
            enabled: true,
            obfuscate: true,
            phrases: vec!["redactme".into()],
            adjustments: vec![ContentAdjustment {
                from: "redactme".into(),
                to: "REDACTED".into(),
            }],
            ..ProviderFilterConfig::default()
        })
        .expect("compiles");

        let expected = [
            (
                "redactme.html",
                "<!DOCTYPE html>\n<html>\n<head><title>REDACTED sample</title></head>\n<body>\n<h1>REDACTED</h1>\n<p>See REDACTED, then REDACTED, then REDACTED again.</p>\n<p>Open notes/REDACTED.txt and REDACTED.js.</p>\n<a href=\"files/REDACTED.txt\">REDACTED</a>\n<!-- REDACTED REDACTED -->\n</body>\n</html>\n",
            ),
            (
                "redactme.txt",
                "REDACTED is the first token.\nREDACTED is the second token.\nBoth appear here: REDACTED and REDACTED.\nThe name on disk is REDACTED.txt.\nLeave redacted alone.\n",
            ),
            (
                "redactme.js",
                "// REDACTED and REDACTED stay in this comment.\nconst label = \"REDACTED\";\nconst shout = \"REDACTED\";\nexport function show(name) {\n  return name + \" REDACTED \" + \"REDACTED\";\n}\nexport const source = \"REDACTED.js\";\n",
            ),
        ];
        for (name, outbound_expected) in expected {
            let path = root.join(name);
            let original = std::fs::read_to_string(&path).expect("fixture");
            let outbound = adjusted(&filter, &original);
            assert_eq!(outbound, outbound_expected, "{name}");
            assert_eq!(count_ascii_keyword(&outbound, "redactme"), 0, "{outbound}");
            assert_eq!(std::fs::read_to_string(&path).expect("disk"), original);
            let extension = name.rsplit_once('.').expect("extension").1;
            assert_eq!(adjusted(&filter, name), format!("REDACTED.{extension}"));
            assert_eq!(
                filter.restore_text(&format!("REDACTED.{extension}")),
                format!("redactme.{extension}")
            );
        }

        let notes = std::fs::read_to_string(root.join("redactme.txt")).expect("notes");
        let restored = filter.restore_text(&adjusted(&filter, &notes));
        assert!(restored.contains("Leave redactme alone."), "{restored}");
    }

    #[test]
    fn provider_reply_puts_redactme_back_in_the_view() {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/provider-filter");
        let filter = ProviderFilter::compile(&ProviderFilterConfig {
            enabled: true,
            obfuscate: true,
            phrases: vec!["redactme".into()],
            adjustments: vec![ContentAdjustment {
                from: "redactme".into(),
                to: "REDACTED".into(),
            }],
            ..ProviderFilterConfig::default()
        })
        .expect("compiles");
        let expected = [
            (
                "redactme.html",
                "<!DOCTYPE html>\n<html>\n<head><title>redactme sample</title></head>\n<body>\n<h1>redactme</h1>\n<p>See redactme, then redactme, then redactme again.</p>\n<p>Open notes/redactme.txt and redactme.js.</p>\n<a href=\"files/redactme.txt\">redactme</a>\n<!-- redactme redactme -->\n</body>\n</html>\n",
            ),
            (
                "redactme.txt",
                "redactme is the first token.\nredactme is the second token.\nBoth appear here: redactme and redactme.\nThe name on disk is redactme.txt.\nLeave redactme alone.\n",
            ),
            (
                "redactme.js",
                "// redactme and redactme stay in this comment.\nconst label = \"redactme\";\nconst shout = \"redactme\";\nexport function show(name) {\n  return name + \" redactme \" + \"redactme\";\n}\nexport const source = \"redactme.js\";\n",
            ),
        ];
        for (name, view_expected) in expected {
            let original = std::fs::read_to_string(root.join(name)).expect("fixture");
            let provider_text = adjusted(&filter, &original);
            let mut view = String::new();
            for chunk in provider_text.as_bytes().chunks(5) {
                view.push_str(std::str::from_utf8(chunk).expect("chunk"));
                let restored = filter.restore_text(&view);
                if restored != view {
                    view = restored;
                }
            }
            assert_eq!(view, view_expected, "{name}");
            assert_eq!(
                std::fs::read_to_string(root.join(name)).expect("disk"),
                original
            );
        }

        let automatic = ProviderFilter::compile(&ProviderFilterConfig {
            enabled: true,
            obfuscate: true,
            phrases: vec!["redactme".into()],
            ..ProviderFilterConfig::default()
        })
        .expect("compiles");
        let notes = std::fs::read_to_string(root.join("redactme.txt")).expect("notes");
        let echoed = adjusted(&automatic, &notes);
        assert!(filter
            .restore_text("Leave redacted alone.")
            .contains("redactme"));
        assert_eq!(
            automatic.restore_text(&echoed).lines().next_back(),
            Some("Leave redacted alone.")
        );
    }

    fn page_filter(mode: PageStandIn, items: &[&str]) -> ProviderFilter {
        ProviderFilter::compile(&ProviderFilterConfig {
            enabled: true,
            page_stand_in: mode,
            page_stand_ins: items.iter().map(|item| (*item).to_string()).collect(),
            ..ProviderFilterConfig::default()
        })
        .expect("page filter")
    }

    fn fetch_request(user: &str, calls: &[(&str, &str, &str)]) -> String {
        let mut messages = vec![serde_json::json!({ "role": "user", "content": user })];
        for (id, name, output) in calls {
            messages.push(serde_json::json!({
                "role": "assistant",
                "content": "",
                "tool_calls": [{
                    "id": id,
                    "type": "function",
                    "function": { "name": name, "arguments": "{}" }
                }]
            }));
            messages.push(serde_json::json!({
                "role": "tool",
                "tool_call_id": id,
                "content": output
            }));
        }
        serde_json::json!({ "messages": messages }).to_string()
    }

    fn tool_content(body: &str, id: &str) -> String {
        let value: serde_json::Value = serde_json::from_str(body).expect("request json");
        value["messages"]
            .as_array()
            .expect("messages")
            .iter()
            .find(|message| message["tool_call_id"] == id)
            .and_then(|message| message["content"].as_str())
            .expect("tool content")
            .to_string()
    }

    fn restore_in_chunks(filter: &ProviderFilter, text: &str) -> String {
        let chars: Vec<char> = text.chars().collect();
        let mut view = String::new();
        for chunk in chars.chunks(3) {
            view.extend(chunk);
            let restored = filter.restore_text(&view);
            if restored != view {
                view = restored;
            }
        }
        view
    }

    #[test]
    fn page_stand_in_rewrites_fetched_page_and_restores_the_words() {
        let filter = page_filter(PageStandIn::Latin, &[]);
        assert!(filter.shows_banner());
        assert!(filter.keeps_html_source());

        let ab = tool_content(
            &adjusted(
                &filter,
                &fetch_request("leave ab", &[("ab", "web_fetch", "ab")]),
            ),
            "ab",
        );
        assert_eq!(ab, "«aer-terra-aer-ignis-aer»");
        assert_eq!(filter.restore_text(&ab), "ab");

        let page = "\
<p>The surgery went well. Later the surgery ended.</p>
<a href=\"https://example.com/surgery\">notes</a>
<img alt=\"surgery photo\">
<!-- surgery note -->
";
        let request = fetch_request(
            "What happened in the surgery?",
            &[
                ("page", "web_fetch", page),
                ("file", "read_file", "The surgery notes stay."),
            ],
        );
        let outbound = adjusted(&filter, &request);
        assert_eq!(adjusted(&filter, &outbound), outbound);
        assert!(outbound.contains("What happened in the surgery?"));
        assert!(outbound.contains("The surgery notes stay."));
        assert!(outbound.contains("\"role\":\"user\""));
        let encoded = tool_content(&outbound, "page");
        let file = tool_content(&outbound, "file");
        assert_eq!(file, "The surgery notes stay.");
        assert!(encoded.contains("<p>"));
        assert!(encoded.contains("https://example.com/surgery"));
        let surgery = tool_content(
            &adjusted(
                &filter,
                &fetch_request("x", &[("s", "web_fetch", "surgery")]),
            ),
            "s",
        );
        assert_eq!(encoded.matches(&surgery).count(), 4, "{encoded}");
        let without_url = encoded.replace("https://example.com/surgery", "");
        assert!(!without_url.contains("surgery"), "{encoded}");
        assert!(!without_url.contains("notes"), "{encoded}");
        assert_eq!(filter.restore_text(&encoded), page);
        assert_eq!(restore_in_chunks(&filter, &encoded), page);

        let spaced = "{\n  \"messages\": [\n    {\"role\": \"user\", \"content\": \"ab stays\"},\n    {\"role\": \"assistant\", \"tool_calls\": [{\"id\": \"c\", \"type\": \"function\", \"function\": {\"name\": \"web_fetch\", \"arguments\": \"{}\"}}]},\n    {\"role\": \"tool\", \"tool_call_id\": \"c\", \"content\": \"ab\"}\n  ]\n}\n";
        let spaced_out = adjusted(&filter, spaced);
        assert!(spaced_out.contains("\"role\": \"user\""));
        assert!(spaced_out.contains("ab stays"));
        assert!(spaced_out.contains(&ab));
        assert!(spaced_out.ends_with('\n'));

        let responses = serde_json::json!({
            "input": [
                {"type": "function_call", "call_id": "call-1", "name": "web_fetch", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "call-1", "output": "ab"},
                {"role": "user", "content": "ab stays"}
            ]
        })
        .to_string();
        let responses_out = adjusted(&filter, &responses);
        let responses_value: serde_json::Value =
            serde_json::from_str(&responses_out).expect("responses");
        assert_eq!(responses_value["input"][1]["output"], ab);
        assert_eq!(responses_value["input"][2]["content"], "ab stays");

        let anthropic = serde_json::json!({
            "messages": [
                {"role": "assistant", "content": [{"type": "tool_use", "id": "call-1", "name": "web_fetch", "input": {}}]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "call-1", "content": [{"type": "text", "text": "ab"}]},
                    {"type": "text", "text": "ab stays"}
                ]}
            ]
        })
        .to_string();
        let anthropic_out = adjusted(&filter, &anthropic);
        let anthropic_value: serde_json::Value =
            serde_json::from_str(&anthropic_out).expect("anthropic");
        assert_eq!(
            anthropic_value["messages"][1]["content"][0]["content"][0]["text"],
            ab
        );
        assert_eq!(
            anthropic_value["messages"][1]["content"][1]["text"],
            "ab stays"
        );

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/provider-filter/redactme.html");
        let original = std::fs::read_to_string(&root).expect("fixture");
        let fixture_out = adjusted(
            &filter,
            &fetch_request("look", &[("html", "web_fetch", &original)]),
        );
        let fixture_page = tool_content(&fixture_out, "html");
        assert!(fixture_page.contains("<html>"));
        assert!(!fixture_page.contains("sample"), "{fixture_page}");
        assert_eq!(filter.restore_text(&fixture_page), original);
        assert_eq!(std::fs::read_to_string(&root).expect("disk"), original);
    }

    #[test]
    fn page_stand_in_accepts_characters_words_or_phrases() {
        let characters = page_filter(PageStandIn::Characters, &["xy"]);
        let words = page_filter(PageStandIn::Words, &["alpha", "beta"]);
        let phrases = page_filter(PageStandIn::Phrases, &["alpha one", "beta two"]);
        let request = fetch_request("ab stays", &[("c", "web_fetch", "ab")]);
        for filter in [&characters, &words, &phrases] {
            let outbound = adjusted(filter, &request);
            assert!(outbound.contains("ab stays"), "{outbound}");
            let page = tool_content(&outbound, "c");
            assert_ne!(page, "ab");
            assert!(page.starts_with('«') && page.ends_with('»'), "{page}");
            assert_eq!(filter.restore_text(&page), "ab");
            assert_eq!(restore_in_chunks(filter, &page), "ab");
        }
        assert_eq!(
            tool_content(&adjusted(&characters, &request), "c"),
            "«xyyxxxxyxyyxxxyxy»"
        );

        let off = ProviderFilter::compile(&ProviderFilterConfig {
            enabled: true,
            ..ProviderFilterConfig::default()
        })
        .expect("off");
        assert!(off.shows_banner());
        assert!(!off.keeps_html_source());
        assert_eq!(adjusted(&off, &request), request);

        let disabled = ProviderFilter::compile(&ProviderFilterConfig {
            enabled: false,
            page_stand_in: PageStandIn::Latin,
            ..ProviderFilterConfig::default()
        })
        .expect("disabled");
        assert!(!disabled.shows_banner());
        assert!(!disabled.keeps_html_source());
        assert_eq!(adjusted(&disabled, &request), request);

        let mixed = ProviderFilter::compile(&ProviderFilterConfig {
            enabled: true,
            page_stand_in: PageStandIn::Latin,
            phrases: vec!["redactme".into()],
            adjustments: vec![ContentAdjustment {
                from: "redactme".into(),
                to: "REDACTED".into(),
            }],
            ..ProviderFilterConfig::default()
        })
        .expect("mixed");
        let mixed_page = "redactme underwent surgery";
        let mixed_out = tool_content(
            &adjusted(
                &mixed,
                &fetch_request("redactme", &[("c", "web_fetch", mixed_page)]),
            ),
            "c",
        );
        assert!(mixed_out.starts_with("REDACTED "), "{mixed_out}");
        assert!(!mixed_out.contains("underwent"), "{mixed_out}");
        assert!(!mixed_out.contains("surgery"), "{mixed_out}");
        assert!(!mixed_out.contains("redactme"), "{mixed_out}");
        assert_eq!(mixed.restore_text(&mixed_out), mixed_page);
        assert!(adjusted(&mixed, &fetch_request("redactme stays", &[])).contains("REDACTED stays"));

        let latin_items = ProviderFilterConfig {
            page_stand_in: PageStandIn::Latin,
            page_stand_ins: vec!["extra".into()],
            ..ProviderFilterConfig::default()
        };
        let error = latin_items.validate().expect_err("latin items").to_string();
        assert!(error.contains("does not accept"));
        assert!(error.contains("\"extra\""));
        let one_word = ProviderFilterConfig {
            page_stand_in: PageStandIn::Words,
            page_stand_ins: vec!["only".into()],
            ..ProviderFilterConfig::default()
        };
        assert!(one_word.validate().is_err());
        let one_char = ProviderFilterConfig {
            page_stand_in: PageStandIn::Characters,
            page_stand_ins: vec!["aa".into()],
            ..ProviderFilterConfig::default()
        };
        assert!(one_char.validate().is_err());
        let bad_phrase = ProviderFilterConfig {
            page_stand_in: PageStandIn::Phrases,
            page_stand_ins: vec!["alpha / beta".into(), "gamma".into()],
            ..ProviderFilterConfig::default()
        };
        let phrase_error = bad_phrase.validate().expect_err("phrase").to_string();
        assert!(phrase_error.contains("\"alpha / beta\""));
    }

    #[test]
    fn outbound_pane_shows_the_provider_page_before_the_exchange() {
        let chat = serde_json::json!({
            "model": "m",
            "messages": [
                {"role": "system", "content": "system rules"},
                {"role": "user", "content": "summarize"},
                {"role": "assistant", "content": "", "tool_calls": [{
                    "id": "c",
                    "type": "function",
                    "function": {"name": "web_fetch", "arguments": "{}"}
                }]},
                {"role": "tool", "tool_call_id": "c", "content": "«aer-terra» <p>stay</p>"},
                {"role": "tool", "tool_call_id": "other", "content": "file body"}
            ],
            "tools": [{
                "type": "function",
                "function": {"name": "web_fetch", "description": "DO_NOT_SHOW_SCHEMA"}
            }]
        });
        let text = outbound_pane_text(chat.to_string().as_bytes());
        assert!(text.starts_with("«aer-terra» <p>stay</p>"), "{text}");
        assert!(text.contains("summarize"));
        assert!(text.contains("file body"));
        assert!(text.contains("system rules"));
        assert!(!text.contains("DO_NOT_SHOW_SCHEMA"), "{text}");
        let page_at = text.find("«aer-terra»").expect("page");
        let user_at = text.find("summarize").expect("user");
        let file_at = text.find("file body").expect("file");
        let system_at = text.find("system rules").expect("system");
        assert!(page_at < user_at && user_at < file_at && file_at < system_at);

        let responses = serde_json::json!({
            "input": [
                {"role": "user", "content": "summarize"},
                {"type": "function_call", "name": "web_fetch", "call_id": "c"},
                {"type": "function_call_output", "call_id": "c", "output": "«page»"}
            ]
        });
        let responses_text = outbound_pane_text(responses.to_string().as_bytes());
        assert!(responses_text.starts_with("«page»"), "{responses_text}");
        assert!(responses_text.contains("summarize"));
        assert!(
            !responses_text.contains("function_call"),
            "{responses_text}"
        );

        let anthropic = serde_json::json!({
            "system": [{"type": "text", "text": "system rules"}],
            "messages": [
                {"role": "assistant", "content": [{
                    "type": "tool_use",
                    "id": "c",
                    "name": "web_fetch",
                    "input": {}
                }]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "c", "content": [
                        {"type": "text", "text": "«page»"}
                    ]},
                    {"type": "text", "text": "summarize"}
                ]}
            ],
            "tools": [{"name": "web_fetch", "description": "DO_NOT_SHOW_SCHEMA"}]
        });
        let anthropic_text = outbound_pane_text(anthropic.to_string().as_bytes());
        assert!(anthropic_text.starts_with("«page»"), "{anthropic_text}");
        assert!(anthropic_text.contains("summarize"));
        assert!(anthropic_text.contains("system rules"));
        assert!(
            !anthropic_text.contains("DO_NOT_SHOW_SCHEMA"),
            "{anthropic_text}"
        );

        assert_eq!(outbound_pane_text(b"not json"), "not json");
    }

    fn image_filter(enabled: bool, placeholder_images: bool) -> ProviderFilter {
        ProviderFilter::compile(&ProviderFilterConfig {
            enabled,
            placeholder_images,
            ..ProviderFilterConfig::default()
        })
        .expect("image filter")
    }

    #[test]
    fn placeholder_images_replace_outbound_pictures_only() {
        let on = image_filter(true, true);
        assert!(on.shows_banner());
        assert!(on.keeps_html_source());
        let idle = image_filter(true, false);
        assert!(idle.shows_banner());
        assert!(!idle.keeps_html_source());
        let disabled = image_filter(false, true);
        assert!(!disabled.shows_banner());
        assert!(!disabled.keeps_html_source());
        let parsed: ProviderFilterConfig = toml::from_str("enabled = true\n").expect("toml");
        assert!(!parsed.placeholder_images);

        let chat = r#"{"zzz":1,"messages":[{"role":"user","content":[{"type":"text","text":"look at this"},{"type":"image_url","image_url":{"url":"data:image/png;base64,iVBORw0KGgo"}}]}]}"#;
        assert_eq!(
            disabled.prepare_request(chat.as_bytes()).expect("disabled"),
            chat.as_bytes()
        );
        let phrase_only = ProviderFilter::compile(&ProviderFilterConfig {
            enabled: true,
            phrases: vec!["secret".into()],
            placeholder_images: false,
            ..ProviderFilterConfig::default()
        })
        .expect("phrase only");
        assert!(phrase_only.shows_banner());
        assert!(!phrase_only.keeps_html_source());
        let kept = String::from_utf8(phrase_only.prepare_request(chat.as_bytes()).expect("kept"))
            .expect("utf8");
        assert!(kept.contains("data:image/png;base64,iVBORw0KGgo"), "{kept}");

        let once =
            String::from_utf8(on.prepare_request(chat.as_bytes()).expect("chat")).expect("utf8");
        assert!(once.contains(r#""zzz":1"#), "{once}");
        assert!(!once.contains("data:image"), "{once}");
        assert!(!once.contains("iVBORw0KGgo"), "{once}");
        let chat_value: serde_json::Value = serde_json::from_str(&once).expect("chat json");
        assert_eq!(
            chat_value["messages"][0]["content"][0]["text"],
            "look at this"
        );
        assert_eq!(chat_value["messages"][0]["content"][1]["type"], "text");
        assert_eq!(chat_value["messages"][0]["content"][1]["text"], "[image]");
        let twice =
            String::from_utf8(on.prepare_request(once.as_bytes()).expect("twice")).expect("utf8");
        assert_eq!(twice, once);
        let pane = outbound_pane_text(once.as_bytes());
        assert!(pane.contains("[image]"), "{pane}");
        assert!(pane.contains("look at this"), "{pane}");
        assert!(!pane.contains("iVBORw0KGgo"), "{pane}");

        let responses = r#"{"input":[{"role":"user","content":[{"type":"input_text","text":"look at this"},{"type":"input_image","image_url":"data:image/png;base64,iVBORw0KGgo"}]}]}"#;
        let responses_out =
            String::from_utf8(on.prepare_request(responses.as_bytes()).expect("responses"))
                .expect("utf8");
        assert!(!responses_out.contains("data:image"), "{responses_out}");
        let responses_value: serde_json::Value =
            serde_json::from_str(&responses_out).expect("responses json");
        assert_eq!(
            responses_value["input"][0]["content"][0]["text"],
            "look at this"
        );
        assert_eq!(
            responses_value["input"][0]["content"][1]["type"],
            "input_text"
        );
        assert_eq!(responses_value["input"][0]["content"][1]["text"], "[image]");
        let responses_pane = outbound_pane_text(responses_out.as_bytes());
        assert!(responses_pane.contains("[image]"), "{responses_pane}");
        assert!(responses_pane.contains("look at this"), "{responses_pane}");

        let anthropic = r#"{"messages":[{"role":"user","content":[{"type":"text","text":"look at this"},{"type":"image","source":{"type":"base64","media_type":"image/png","data":"iVBORw0KGgo"}},{"type":"tool_use","id":"1","name":"view_image","input":{}},{"type":"image","name":"label"}]}]}"#;
        let anthropic_out =
            String::from_utf8(on.prepare_request(anthropic.as_bytes()).expect("anthropic"))
                .expect("utf8");
        assert!(!anthropic_out.contains("iVBORw0KGgo"), "{anthropic_out}");
        let anthropic_value: serde_json::Value =
            serde_json::from_str(&anthropic_out).expect("anthropic json");
        let blocks = &anthropic_value["messages"][0]["content"];
        assert_eq!(blocks[0]["text"], "look at this");
        assert_eq!(blocks[1]["type"], "text");
        assert_eq!(blocks[1]["text"], "[image]");
        assert_eq!(blocks[2]["type"], "tool_use");
        assert_eq!(blocks[2]["name"], "view_image");
        assert_eq!(blocks[3]["type"], "image");
        assert_eq!(blocks[3]["name"], "label");

        let page = "See <img src=\"https://cdn.example/a.png\" alt=\"surgery\"> and <IMAGE src=\"https://cdn.example/b.png\"> and ![shot](data:image/png;base64,AAAA) and data:image/gif;base64,BBBB and https://example.com/surgery later. start <img src=\"nope\" end";
        let fetched = fetch_request("hello", &[("c", "web_fetch", page)]);
        let fetched_out =
            String::from_utf8(on.prepare_request(fetched.as_bytes()).expect("page")).expect("utf8");
        let fetched_page = tool_content(&fetched_out, "c");
        assert_eq!(
            fetched_page,
            "See [image] and [image] and [image] and [image] and https://example.com/surgery later. start <img src=\"nope\" end"
        );
        assert_eq!(
            placeholder_in_text("<img alt=\"a>b\" src=\"z\"> tail"),
            "[image] tail"
        );
        let fetched_twice = String::from_utf8(
            on.prepare_request(fetched_out.as_bytes())
                .expect("page twice"),
        )
        .expect("utf8");
        assert_eq!(tool_content(&fetched_twice, "c"), fetched_page);

        let hidden = ProviderFilter::compile(&ProviderFilterConfig {
            enabled: true,
            placeholder_images: true,
            phrases: vec!["NEEDLE".into()],
            ..ProviderFilterConfig::default()
        })
        .expect("needle");
        let only_image = r#"{"messages":[{"role":"user","content":[{"type":"text","text":"hello"},{"type":"image_url","image_url":{"url":"data:image/png;base64,NEEDLE"}}]}]}"#;
        let hidden_out = String::from_utf8(
            hidden
                .prepare_request(only_image.as_bytes())
                .expect("image phrase does not trip the request"),
        )
        .expect("utf8");
        assert!(!hidden_out.contains("NEEDLE"), "{hidden_out}");
        assert!(hidden_out.contains("[image]"), "{hidden_out}");
        let alt_page = fetch_request(
            "hello",
            &[(
                "c",
                "web_fetch",
                "before <img alt=\"NEEDLE\" src=\"https://cdn.example/a.png\"> after",
            )],
        );
        let alt_out = String::from_utf8(
            hidden
                .prepare_request(alt_page.as_bytes())
                .expect("alt phrase does not trip the request"),
        )
        .expect("utf8");
        let alt_text = tool_content(&alt_out, "c");
        assert_eq!(alt_text, "before [image] after");
        assert!(!alt_out.contains("NEEDLE"), "{alt_out}");

        let armed = ProviderFilter::compile(&ProviderFilterConfig {
            enabled: true,
            placeholder_images: true,
            phrases: vec!["NEEDLE".into()],
            ..ProviderFilterConfig::default()
        })
        .expect("armed");
        let error = armed
            .prepare_request(br#"{"messages":[{"role":"user","content":"NEEDLE"}]}"#)
            .unwrap_err();
        assert!(matches!(error, ProviderError::FilterStopped { .. }));

        let page_and_image = ProviderFilter::compile(&ProviderFilterConfig {
            enabled: true,
            placeholder_images: true,
            page_stand_in: PageStandIn::Latin,
            ..ProviderFilterConfig::default()
        })
        .expect("both");
        let mixed = fetch_request(
            "hello stays",
            &[(
                "c",
                "web_fetch",
                "before <img src=\"https://cdn.example/a.png\" alt=\"surgery\"> after",
            )],
        );
        let mixed_out = tool_content(
            &String::from_utf8(
                page_and_image
                    .prepare_request(mixed.as_bytes())
                    .expect("mixed"),
            )
            .expect("utf8"),
            "c",
        );
        assert!(mixed_out.contains("[image]"), "{mixed_out}");
        assert!(!mixed_out.contains("cdn.example"), "{mixed_out}");
        assert!(!mixed_out.contains("before"), "{mixed_out}");
        assert!(!mixed_out.contains("surgery"), "{mixed_out}");
        assert!(mixed_out.contains('«'), "{mixed_out}");
        let user_out = String::from_utf8(
            page_and_image
                .prepare_request(fetch_request("hello stays", &[]).as_bytes())
                .expect("user"),
        )
        .expect("utf8");
        assert!(user_out.contains("hello stays"), "{user_out}");
    }

    #[test]
    fn obfuscation_patterns_rewrite_a_word_and_never_trip() {
        let rewrite = ProviderFilter::compile(&ProviderFilterConfig {
            enabled: true,
            obfuscation_patterns: vec!["secret".into()],
            ..ProviderFilterConfig::default()
        })
        .expect("rewrite");
        assert!(rewrite.shows_banner());
        assert!(rewrite.keeps_html_source());
        let out = adjusted(&rewrite, "See SECRET now");
        assert_eq!(out, "See ca-kw-01 now");
        assert_eq!(rewrite.restore_text(&out), "See secret now");
        assert!(rewrite.prepare_request(b"See SECRET now").is_ok());
        let twice = adjusted(&rewrite, &out);
        assert_eq!(twice, out);
        let mut carry = Vec::new();
        assert!(rewrite
            .observe_response(&mut carry, b"SECRET came back")
            .is_ok());

        let disabled = ProviderFilter::compile(&ProviderFilterConfig {
            enabled: false,
            obfuscation_patterns: vec!["secret".into()],
            ..ProviderFilterConfig::default()
        })
        .expect("disabled");
        assert!(!disabled.shows_banner());
        assert!(!disabled.keeps_html_source());
        assert_eq!(
            disabled
                .prepare_request(b"See SECRET now")
                .expect("unchanged"),
            b"See SECRET now"
        );
        assert!(!ProviderFilterConfig {
            obfuscation_patterns: vec!["secret".into()],
            ..ProviderFilterConfig::default()
        }
        .is_disabled());

        let kill = ProviderFilter::compile(&ProviderFilterConfig {
            enabled: true,
            phrases: vec!["A-123".into()],
            ..ProviderFilterConfig::default()
        })
        .expect("kill");
        assert!(!kill.keeps_html_source());
        assert_eq!(adjusted(&kill, "secret stays"), "secret stays");
        let killed = kill.prepare_request(b"ticket A-123").unwrap_err();
        assert_eq!(
            killed,
            ProviderError::FilterStopped {
                rule: "phrase-1".into()
            }
        );

        let both = ProviderFilter::compile(&ProviderFilterConfig {
            enabled: true,
            phrases: vec!["A-123".into()],
            obfuscation_patterns: vec!["secret".into()],
            ..ProviderFilterConfig::default()
        })
        .expect("both");
        assert_eq!(adjusted(&both, "secret only"), "ca-kw-01 only");
        assert!(both.prepare_request(b"secret only").is_ok());
        let mut carry = Vec::new();
        assert!(both
            .observe_response(&mut carry, b"secret came back")
            .is_ok());
        let response = both.observe_response(&mut carry, b"A-123").unwrap_err();
        assert_eq!(
            response,
            ProviderError::FilterStopped {
                rule: "phrase-1".into()
            }
        );

        let overlap = ProviderFilter::compile(&ProviderFilterConfig {
            enabled: true,
            phrases: vec!["secret".into()],
            obfuscation_patterns: vec!["secret".into()],
            ..ProviderFilterConfig::default()
        })
        .expect("overlap");
        assert_eq!(adjusted(&overlap, "SECRET"), "ca-kw-01");
        assert!(overlap.prepare_request(b"SECRET").is_ok());
        let mut carry = Vec::new();
        let echoed = overlap.observe_response(&mut carry, b"secret").unwrap_err();
        assert_eq!(
            echoed,
            ProviderError::FilterStopped {
                rule: "phrase-1".into()
            }
        );

        let chosen = ProviderFilter::compile(&ProviderFilterConfig {
            enabled: true,
            obfuscation_patterns: vec!["secret".into()],
            adjustments: vec![ContentAdjustment {
                from: "secret".into(),
                to: "REDACTED".into(),
            }],
            ..ProviderFilterConfig::default()
        })
        .expect("chosen");
        assert_eq!(adjusted(&chosen, "SECRET"), "REDACTED");

        let numbered = ProviderFilter::compile(&ProviderFilterConfig {
            enabled: true,
            obfuscate: true,
            phrases: vec!["A-123".into()],
            obfuscation_patterns: vec!["secret".into()],
            ..ProviderFilterConfig::default()
        })
        .expect("numbered");
        assert_eq!(
            adjusted(&numbered, "secret then A-123"),
            "ca-kw-02 then ca-kw-01"
        );
    }

    fn count_ascii_keyword(text: &str, keyword: &str) -> usize {
        let folded = text.as_bytes();
        let needle = keyword.as_bytes();
        let mut count = 0;
        let mut index = 0;
        while index + needle.len() <= folded.len() {
            if folded[index..index + needle.len()]
                .iter()
                .zip(needle)
                .all(|(byte, expected)| byte.eq_ignore_ascii_case(expected))
            {
                count += 1;
                index += needle.len();
            } else {
                index += 1;
            }
        }
        count
    }

    #[test]
    fn legacy_file_still_rewrites_trips_and_keeps_url_prefixes() {
        let config: ProviderFilterConfig = toml::from_str(
            "\
enabled = true
obfuscate = true
phrases = [\"secret\"]
urls = [\"http://stop.example/hidden\"]
page_stand_in = \"latin\"
[[adjustments]]
from = \"http://files.example/docs\"
to = \"http://go.example/docs\"
",
        )
        .expect("legacy file parses");
        let filter = ProviderFilter::compile(&config).expect("legacy file compiles");
        assert_eq!(adjusted(&filter, "secret"), "ca-kw-01");
        assert_eq!(
            adjusted(&filter, "http://stop.example/hidden/more?q=1"),
            "http://ca-r01.invalid/more?q=1"
        );
        assert_eq!(
            adjusted(&filter, "http://files.example/docs/child"),
            "http://go.example/docs/child"
        );
        let page = "alpha underwent surgery";
        let body = tool_content(
            &adjusted(&filter, &fetch_request("keep", &[("c", "web_fetch", page)])),
            "c",
        );
        assert!(!body.contains("underwent"), "{body}");
        assert_eq!(filter.restore_text(&body), page);
        let error = filter
            .observe_response(&mut Vec::new(), b"still secret")
            .unwrap_err();
        assert_eq!(
            error,
            ProviderError::FilterStopped {
                rule: "phrase-1".into()
            }
        );
        assert!(!error.to_string().contains("secret"));
        let report = format_check(&config, None).expect("check");
        assert!(report.contains("pages latin"), "{report}");
        assert!(report.contains("prefix on (from adjustments)"), "{report}");
    }

    #[test]
    fn hide_url_is_exact_unless_prefix_is_on() {
        let exact: ProviderFilterConfig = toml::from_str(
            "\
enabled = true
[[hide.urls]]
from = \"http://stop.com\"
to = \"http://go.com\"
",
        )
        .expect("exact hide url parses");
        let filter = ProviderFilter::compile(&exact).expect("compiles");
        assert_eq!(
            adjusted(&filter, "see http://stop.com now"),
            "see http://go.com now"
        );
        assert_eq!(
            adjusted(&filter, "http://stop.com/child"),
            "http://stop.com/child"
        );
        assert!(filter.admit_outbound(b"http://stop.com").is_ok());
        assert!(filter
            .observe_response(&mut Vec::new(), b"http://stop.com")
            .is_ok());
        assert!(!filter.is_tripped());

        let prefixed: ProviderFilterConfig = toml::from_str(
            "\
enabled = true
[[hide.urls]]
from = \"http://stop.com\"
to = \"http://go.com\"
prefix = true
",
        )
        .expect("prefix hide url parses");
        let filter = ProviderFilter::compile(&prefixed).expect("compiles");
        assert_eq!(
            adjusted(&filter, "http://stop.com/child"),
            "http://go.com/child"
        );
        assert!(filter
            .observe_response(&mut Vec::new(), b"http://stop.com/child")
            .is_ok());
    }

    #[test]
    fn the_same_word_in_stop_and_hide_fails_unless_both_lists_are_legacy() {
        let clash: ProviderFilterConfig = toml::from_str(
            "\
[stop]
phrases = [\"secret\"]
[hide]
words = [\"secret\"]
",
        )
        .expect("parses");
        let error = clash.validate().unwrap_err().to_string();
        assert!(error.contains("stop and hide"), "{error}");
        assert!(error.contains("\"secret\""), "{error}");

        let new_stop_old_hide: ProviderFilterConfig = toml::from_str(
            "\
obfuscation_patterns = [\"secret\"]
[stop]
phrases = [\"secret\"]
",
        )
        .expect("parses");
        let error = new_stop_old_hide.validate().unwrap_err().to_string();
        assert!(error.contains("\"secret\""), "{error}");

        let old_stop_new_hide: ProviderFilterConfig = toml::from_str(
            "\
phrases = [\"secret\"]
[hide]
words = [\"secret\"]
",
        )
        .expect("parses");
        let error = old_stop_new_hide.validate().unwrap_err().to_string();
        assert!(error.contains("\"secret\""), "{error}");

        let both_keys: ProviderFilterConfig = toml::from_str(
            "\
phrases = [\"secret\"]
[stop]
phrases = [\"other\"]
",
        )
        .expect("parses");
        let error = both_keys.validate().unwrap_err().to_string();
        assert!(error.contains("phrases"), "{error}");
        assert!(error.contains("stop.phrases"), "{error}");

        let legacy: ProviderFilterConfig = toml::from_str(
            "\
enabled = true
phrases = [\"secret\"]
obfuscation_patterns = [\"secret\"]
",
        )
        .expect("legacy overlap parses");
        let filter = ProviderFilter::compile(&legacy).expect("legacy overlap compiles");
        assert_eq!(adjusted(&filter, "secret"), "ca-kw-01");
        let error = filter
            .observe_response(&mut Vec::new(), b"secret")
            .unwrap_err();
        assert_eq!(
            error,
            ProviderError::FilterStopped {
                rule: "phrase-1".into()
            }
        );
        assert!(!error.to_string().contains("secret"));
        let report = format_check(&legacy, None).expect("check");
        assert!(
            report.contains("masked stop (listed in phrases and obfuscation_patterns)"),
            "{report}"
        );
        assert!(report.contains("\"secret\""), "{report}");
    }

    #[test]
    fn pages_list_round_trips_and_characters_still_load() {
        let words: ProviderFilterConfig = toml::from_str(
            "\
enabled = true
pages = [\"alpha\", \"beta\"]
",
        )
        .expect("word list parses");
        let filter = ProviderFilter::compile(&words).expect("word list compiles");
        let request = fetch_request("ab stays", &[("c", "web_fetch", "ab")]);
        let page = tool_content(&adjusted(&filter, &request), "c");
        assert_ne!(page, "ab");
        assert!(page.starts_with('«') && page.ends_with('»'), "{page}");
        assert_eq!(filter.restore_text(&page), "ab");
        let report = format_check(&words, None).expect("check");
        assert!(report.contains("pages 2 words"), "{report}");

        let characters: ProviderFilterConfig = toml::from_str(
            "\
enabled = true
page_stand_in = \"characters\"
page_stand_ins = [\"xy\"]
",
        )
        .expect("characters parse");
        let filter = ProviderFilter::compile(&characters).expect("characters compile");
        let page = tool_content(
            &adjusted(
                &filter,
                &fetch_request("ab stays", &[("c", "web_fetch", "ab")]),
            ),
            "c",
        );
        assert_eq!(page, "«xyyxxxxyxyyxxxyxy»");
        assert_eq!(filter.restore_text(&page), "ab");
    }

    #[test]
    fn check_names_stop_send_and_the_scan_origin() {
        let config: ProviderFilterConfig = toml::from_str(
            "\
enabled = true
[stop]
phrases = [\"secret\", \"http://stop.com/stop\"]
",
        )
        .expect("parses");
        let stopped = format_check(&config, Some("see secret")).expect("stop check");
        assert!(
            stopped.contains("would stop on \"secret\", in the request"),
            "{stopped}"
        );
        assert!(!stopped.contains("would send:"), "{stopped}");
        let sent = format_check(&config, Some("hello")).expect("send check");
        assert!(sent.contains("would send: hello"), "{sent}");
        let named = format_check(&config, None).expect("names");
        assert!(
            named.contains("substring"),
            "a URL left in phrases stays a substring: {named}"
        );

        let fetched = ProviderFilter::compile(&config).expect("compiles");
        let error = fetched.admit_fetched_page(b"page secret").unwrap_err();
        assert_eq!(
            error,
            ProviderError::FilterStopped {
                rule: "phrase-1".into()
            }
        );
        assert!(!error.to_string().contains("secret"));
        assert_eq!(
            fetched.local_stop_notice().as_deref(),
            Some("stopped on \"secret\", in the fetched page")
        );

        let reply = ProviderFilter::compile(&config).expect("compiles");
        let error = reply
            .observe_response(&mut Vec::new(), b"secret")
            .unwrap_err();
        assert!(!error.to_string().contains("secret"));
        assert_eq!(
            reply.local_stop_notice().as_deref(),
            Some("stopped on \"secret\", in the reply")
        );

        let masked: ProviderFilterConfig = toml::from_str(
            "\
enabled = true
[[stop.phrases]]
text = \"secret\"
mask = true
",
        )
        .expect("masked stop parses");
        let filter = ProviderFilter::compile(&masked).expect("masked stop compiles");
        assert_eq!(adjusted(&filter, "secret"), "ca-kw-01");
        let sent = format_check(&masked, Some("secret")).expect("masked sample");
        assert!(sent.contains("would send: ca-kw-01"), "{sent}");
        let error = filter
            .observe_response(&mut Vec::new(), b"secret")
            .unwrap_err();
        assert_eq!(
            error,
            ProviderError::FilterStopped {
                rule: "phrase-1".into()
            }
        );
        assert!(!error.to_string().contains("secret"));

        let idle: ProviderFilterConfig = toml::from_str("enabled = true\n").expect("enabled only");
        let report = format_check(&idle, None).expect("idle check");
        assert!(report.contains("filter on, nothing armed"), "{report}");
        let filter = ProviderFilter::compile(&idle).expect("idle compiles");
        assert!(filter.shows_banner());
    }

    #[test]
    fn command_report_offers_filter_on_until_the_process_is_armed() {
        let off = ProviderFilter::compile(&ProviderFilterConfig::default()).expect("off compiles");
        let page = command_report(&off);
        assert_eq!(
            page,
            "filter off\n/filter on turns it on for this process.\n"
        );

        let listed: ProviderFilterConfig =
            toml::from_str("phrases = [\"secret\"]\n").expect("phrase parses");
        let listed = ProviderFilter::compile(&listed).expect("listed compiles");
        let page = command_report(&listed);
        assert!(page.starts_with("filter off\n"), "{page}");
        assert!(page.contains("stop phrase \"secret\""), "{page}");
        assert!(
            page.ends_with("/filter on turns it on for this process.\n"),
            "{page}"
        );
        assert_eq!(page.matches("filter off").count(), 1, "{page}");

        let armed: ProviderFilterConfig =
            toml::from_str("enabled = true\nphrases = [\"secret\"]\n").expect("armed parses");
        let armed = ProviderFilter::compile(&armed).expect("armed compiles");
        let page = command_report(&armed);
        assert!(page.contains("secret"), "{page}");
        assert!(!page.contains("/filter on"), "{page}");

        let idle: ProviderFilterConfig = toml::from_str("enabled = true\n").expect("idle parses");
        let idle = ProviderFilter::compile(&idle).expect("idle compiles");
        assert_eq!(command_report(&idle), "filter on, nothing armed\n");

        armed
            .observe_response(&mut Vec::new(), b"secret")
            .expect_err("reply trips");
        let page = command_report(&armed);
        assert!(armed.is_tripped());
        assert!(!page.contains("/filter on"), "{page}");
    }

    #[test]
    fn arm_loaded_enables_this_process_and_keeps_the_first_rules() {
        with_process_filter(|| {
            let _restore = ProcessFilterRestore::capture();
            let config: ProviderFilterConfig =
                toml::from_str("phrases = [\"secret\"]\n").expect("phrase parses");
            replace_process_filter(
                ProviderFilter::compile(&config).expect("startup filter compiles"),
            );
            assert!(!current().enabled);

            let status = arm_loaded(&config).expect("arm");
            assert_eq!(
                status,
                "filter loaded: stop 1 word. Restart ca to change it."
            );
            assert!(current().enabled);
            assert!(current().watches());
            assert!(
                current().report().contains("secret"),
                "{}",
                current().report()
            );

            let again = arm_loaded(&config).expect("second arm");
            assert_eq!(again, "filter already on: stop 1 word.");

            let other: ProviderFilterConfig =
                toml::from_str("phrases = [\"other-word\"]\n").expect("other parses");
            let stayed = arm_loaded(&other).expect("different rules stay off");
            assert_eq!(stayed, "filter already on: stop 1 word.");
            let active = current();
            let report = active.report();
            assert!(report.contains("secret"), "{report}");
            assert!(!report.contains("other-word"), "{report}");
        });
    }

    #[test]
    fn arm_loaded_with_nothing_configured_stays_quiet() {
        with_process_filter(|| {
            let _restore = ProcessFilterRestore::capture();
            let empty = ProviderFilterConfig::default();
            replace_process_filter(ProviderFilter::compile(&empty).expect("empty compiles"));
            assert!(!current().enabled);
            let status = arm_loaded(&empty).expect("arm");
            assert_eq!(status, "filter on, nothing armed");
            assert!(current().enabled);
            assert!(current().shows_banner());
            assert_eq!(banner_detail(), "nothing armed");
            assert_eq!(
                arm_loaded(&empty).expect("again"),
                "filter already on, nothing armed"
            );
        });
    }

    #[test]
    fn arm_loaded_leaves_a_tripped_session_stopped() {
        with_process_filter(|| {
            let _restore = ProcessFilterRestore::capture();
            let config: ProviderFilterConfig =
                toml::from_str("phrases = [\"secret\"]\n").expect("phrase parses");
            replace_process_filter(
                ProviderFilter::compile(&config).expect("startup filter compiles"),
            );
            arm_loaded(&config).expect("arm");
            current()
                .observe_response(&mut Vec::new(), b"secret")
                .expect_err("reply trips");
            assert!(current().is_tripped());

            let other: ProviderFilterConfig =
                toml::from_str("phrases = [\"other-word\"]\n").expect("other parses");
            let status = arm_loaded(&other).expect("trip stays");
            assert!(status.contains("stays stopped"), "{status}");
            assert!(status.contains("secret"), "{status}");
            assert!(current().is_tripped());
            let active = current();
            let report = active.report();
            assert!(report.contains("secret"), "{report}");
            assert!(!report.contains("other-word"), "{report}");
        });
    }

    #[test]
    fn prepare_page_text_rewrites_hide_words_and_page_stand_ins_on_raw_html() {
        let config: ProviderFilterConfig = toml::from_str(
            "\
enabled = true
[hide]
words = [\"iPhone\"]
[pages]
stand_in = \"latin\"
",
        )
        .expect("parses");
        let filter = ProviderFilter::compile(&config).expect("compiles");
        let page = "<html><body><h1>iPhone</h1><p>See iphone and widgets</p></body></html>";
        let sent = filter.prepare_page_text(page).expect("page sends");
        assert!(!sent.contains("iPhone"), "{sent}");
        assert!(!sent.contains("iphone"), "{sent}");
        assert!(sent.contains("ca-kw-"), "{sent}");
        assert!(sent.contains("<html>"), "{sent}");
        assert!(sent.contains('«'), "{sent}");
        let restored = filter.restore_text(&sent);
        assert!(restored.contains("iPhone"), "{restored}");
    }

    #[test]
    fn prepare_page_text_keeps_tags_and_reports_stop_on_file_content() {
        let stop: ProviderFilterConfig = toml::from_str(
            "\
enabled = true
[stop]
phrases = [\"secret\"]
",
        )
        .expect("parses");
        let filter = ProviderFilter::compile(&stop).expect("compiles");
        let error = filter
            .prepare_page_text("<p>page secret here</p>")
            .unwrap_err();
        assert_eq!(
            error,
            ProviderError::FilterStopped {
                rule: "phrase-1".into()
            }
        );
        let report = format_check_page(&stop, "<p>page secret here</p>").expect("stop report");
        assert!(
            report.contains("would stop on \"secret\", in the request"),
            "{report}"
        );
        assert!(!report.contains("would send:"), "{report}");
    }

    #[test]
    fn format_check_page_sends_rewritten_html_and_truncates_long_bodies() {
        let config: ProviderFilterConfig = toml::from_str(
            "\
enabled = true
[hide]
words = [\"iPhone\"]
[pages]
stand_in = \"latin\"
",
        )
        .expect("parses");
        let report = format_check_page(&config, "<p>iPhone Pro</p>").expect("send report");
        let send = report
            .split("would send: ")
            .nth(1)
            .expect("would send line");
        assert!(!send.contains("iPhone"), "{report}");
        assert!(send.contains("<p>"), "{report}");
        assert!(send.contains("ca-kw-"), "{report}");

        let long = "x".repeat(OUTBOUND_PANE_TEXT_LIMIT + 64);
        let truncated = format_check_page(&config, &long).expect("truncated report");
        assert!(truncated.contains("[truncated]"), "{truncated}");
    }

    #[test]
    fn cli_overlay_adds_hide_words_and_pages_for_one_run() {
        let mut config = ProviderFilterConfig::default();
        apply_cli_overlay(&mut config, &["iPhone".into()], true, Some("latin")).expect("overlay");
        assert!(config.enabled);
        assert_eq!(config.hide.words, vec!["iPhone".to_string()]);
        assert!(config.hide.all_urls);
        assert_eq!(config.pages, PagesSetting::Latin);
        assert_eq!(config.page_stand_in, PageStandIn::Off);

        let mut legacy: ProviderFilterConfig =
            toml::from_str("obfuscation_patterns = [\"Acme\"]\n").expect("legacy parses");
        apply_cli_overlay(&mut legacy, &["Beta".into()], false, None).expect("legacy overlay");
        assert_eq!(
            legacy.obfuscation_patterns,
            vec!["Acme".to_string(), "Beta".to_string()]
        );
        assert!(legacy.hide.words.is_empty());
        assert!(!legacy.hide.all_urls);

        let mut both: ProviderFilterConfig =
            toml::from_str("page_stand_in = \"latin\"\n").expect("legacy pages");
        apply_cli_overlay(&mut both, &[], false, Some("off")).expect("pages overlay");
        assert_eq!(both.pages, PagesSetting::Off);
        assert_eq!(both.page_stand_in, PageStandIn::Off);

        assert!(parse_pages_cli("").is_err());
        assert_eq!(parse_pages_cli("off").expect("off"), PagesSetting::Off);
        assert_eq!(
            parse_pages_cli("alpha, beta").expect("words"),
            PagesSetting::Words(vec!["alpha".into(), "beta".into()])
        );
    }

    #[test]
    fn hide_all_urls_preserves_builtin_tool_schema_identifiers() {
        let config: ProviderFilterConfig = toml::from_str(
            "enabled = true\n[hide]\nall_urls = true\n[pages]\nstand_in = \"latin\"\nimages = true\n",
        )
        .unwrap();
        let filter = ProviderFilter::compile(&config).unwrap();
        let tools = crate::agent::tools::default_tool_defs();
        let original = serde_json::json!({
            "model": "deepseek-v4-pro",
            "messages": [{"role": "user", "content": "See https://example.test/private/page"}],
            "tools": tools,
            "stream": true
        });
        let sent = filter
            .prepare_request(&serde_json::to_vec(&original).unwrap())
            .unwrap();
        let sent: serde_json::Value = serde_json::from_slice(&sent).unwrap();
        let read_file = |value: &serde_json::Value| {
            value["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|tool| tool["function"]["name"] == "read_file")
                .unwrap()["function"]["parameters"]
                .clone()
        };
        let schema = read_file(&original);
        assert!(schema["$schema"].is_string());
        assert_eq!(read_file(&sent), schema);
        assert!(!sent["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("example.test"));
        assert_eq!(
            filter.restore_value(&sent)["messages"],
            original["messages"]
        );
    }

    #[test]
    fn hide_all_urls_preserves_schema_validation_and_hides_annotations() {
        let config: ProviderFilterConfig = toml::from_str(
            "enabled = true\n[hide]\nall_urls = true\n[pages]\nstand_in = \"latin\"\n",
        )
        .unwrap();
        let filter = ProviderFilter::compile(&config).unwrap();
        let schema = serde_json::json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "$id": "https://schema.test/schema",
            "type": "object",
            "description": "See https://annotation.test/private/page",
            "properties": {
                "description": {
                    "$ref": "https://schema.test/definition",
                    "enum": ["https://validation.test/enum"],
                    "const": {"description": "https://validation.test/constant"},
                    "default": {"title": "https://validation.test/default"},
                    "description": "See https://annotation.test/property"
                },
                "const": {"title": "See https://annotation.test/title"}
            },
            "$defs": {"title": {"const": "https://validation.test/definition"}}
        });
        for wrapper in [
            serde_json::json!({"tools": [{"function": {"parameters": schema}}]}),
            serde_json::json!({"tools": [{"input_schema": schema}]}),
            serde_json::json!({"text": {"format": {"schema": schema}}}),
        ] {
            let sent = filter
                .prepare_request(&serde_json::to_vec(&wrapper).unwrap())
                .unwrap();
            let sent: serde_json::Value = serde_json::from_slice(&sent).unwrap();
            let text = sent.to_string();
            assert!(!text.contains("annotation.test"), "{sent}");
            assert!(text.contains("validation.test/constant"), "{sent}");
            assert!(text.contains("validation.test/enum"), "{sent}");
            assert!(text.contains("schema.test/schema"), "{sent}");
            assert_eq!(filter.restore_value(&sent), wrapper);
        }
        let body = serde_json::json!({
            "messages": [{"role": "assistant", "tool_calls": [{"function": {
                "name": "read_file", "arguments": "{\"schema\":\"https://content.test/private\",\"description\":\"https://content.test/other\"}"
            }}]}]
        });
        let sent = filter
            .prepare_request(&serde_json::to_vec(&body).unwrap())
            .unwrap();
        assert!(!String::from_utf8_lossy(&sent).contains("content.test"));
    }

    #[test]
    fn hide_all_urls_rewrites_page_urls_and_restores_them() {
        let config: ProviderFilterConfig = toml::from_str(
            "\
enabled = true
[hide]
words = [\"iPhone\"]
all_urls = true
[pages]
stand_in = \"latin\"
",
        )
        .expect("parses");
        let filter = ProviderFilter::compile(&config).expect("compiles");
        let page = "<a href=\"https://en.wikipedia.org/wiki/IPhone_16_Pro\">iPhone</a> see https://example.com/a and https://example.com/a again.";
        let sent = filter.prepare_page_text(page).expect("page sends");
        assert!(!sent.contains("wikipedia.org"), "{sent}");
        assert!(!sent.contains("example.com"), "{sent}");
        assert!(!sent.contains("iPhone"), "{sent}");
        assert!(sent.contains("https://ca-r01.invalid"), "{sent}");
        assert!(sent.contains("https://ca-r02.invalid"), "{sent}");
        // The authority is redacted and path words are tokenized by the page
        // stand-in, while the URL shape (slashes and the numeric id) survives.
        assert!(sent.contains("https://ca-r01.invalid/"), "{sent}");
        assert!(!sent.contains("/wiki/"), "{sent}");
        assert!(sent.contains("_16_"), "{sent}");
        assert!(sent.contains("https://ca-r02.invalid/"), "{sent}");
        // same URL keeps one stand-in
        assert_eq!(sent.matches("https://ca-r02.invalid").count(), 2, "{sent}");
        let restored = filter.restore_text(&sent);
        assert!(
            restored.contains("https://en.wikipedia.org/wiki/"),
            "{restored}"
        );
        assert!(restored.contains("16_Pro"), "{restored}");
        assert!(restored.contains("https://example.com/a"), "{restored}");
        assert!(restored.contains("iPhone"), "{restored}");

        let report = format_check_page(&config, page).expect("report");
        assert!(
            report.contains("hide urls") || report.contains("hide "),
            "{report}"
        );
        assert!(!report.contains("wikipedia.org"), "{report}");
    }

    #[test]
    fn page_stand_in_preserves_structural_attributes() {
        let config: ProviderFilterConfig = toml::from_str(
            "\
enabled = true
[pages]
stand_in = \"latin\"
",
        )
        .expect("parses");
        let filter = ProviderFilter::compile(&config).expect("compiles");
        let page = "<a class=\"structItem--thread\" rel=\"next\" id=\"thread-65\" data-author=\"M. Davis\">The thread</a>";
        let sent = filter.prepare_page_text(page).expect("page sends");
        // class, rel, and id stay verbatim so selectors and link detection work.
        assert!(sent.contains("structItem--thread"), "{sent}");
        assert!(sent.contains("rel=\"next\""), "{sent}");
        assert!(sent.contains("id=\"thread-65\""), "{sent}");
        // Content attributes and text are still tokenized.
        assert!(!sent.contains("M. Davis"), "{sent}");
        assert!(!sent.contains("The thread"), "{sent}");
        assert!(sent.contains('«'), "{sent}");
    }

    struct ProcessFilterRestore {
        filter: Arc<ProviderFilter>,
        notice: Option<String>,
        force: bool,
        skip: bool,
    }

    impl ProcessFilterRestore {
        fn capture() -> Self {
            Self {
                filter: current(),
                notice: take_process_notice(),
                force: force_flag().load(Ordering::SeqCst),
                skip: skip_install_flag().load(Ordering::SeqCst),
            }
        }
    }

    impl Drop for ProcessFilterRestore {
        fn drop(&mut self) {
            force_flag().store(self.force, Ordering::SeqCst);
            skip_install_flag().store(self.skip, Ordering::SeqCst);
            {
                let mut slot = process_slot()
                    .write()
                    .unwrap_or_else(|poison| poison.into_inner());
                *slot = Arc::clone(&self.filter);
            }
            *notice_slot()
                .lock()
                .unwrap_or_else(|poison| poison.into_inner()) = self.notice.clone();
        }
    }

    fn replace_process_filter(filter: ProviderFilter) {
        let mut slot = process_slot()
            .write()
            .unwrap_or_else(|poison| poison.into_inner());
        *slot = Arc::new(filter);
        let _ = take_process_notice();
    }
}
