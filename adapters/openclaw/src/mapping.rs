//! `OpenClaw` Memory.md 来源映射。

use std::str::FromStr as _;

use jc_memory_protocol::{
    CognitiveFragment, FragmentContent, FragmentId, FragmentKind, FragmentSource, MemoryScope,
    Metadata, Perspective, SourceType, ValidityStatus,
};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use crate::{OpenClawConfig, OpenClawDocument, OpenClawError};

pub(crate) struct MappedDocument {
    pub(crate) document_sha256: String,
    pub(crate) sections: Vec<MappedSection>,
}

pub(crate) struct MappedSection {
    pub(crate) heading: Option<String>,
    pub(crate) line_start: usize,
    pub(crate) line_end: usize,
    pub(crate) fragment: CognitiveFragment,
}

struct SectionRange {
    heading: Option<String>,
    start: usize,
    end: usize,
}

pub(crate) fn map_document(
    document: &OpenClawDocument,
    config: &OpenClawConfig,
    scope: &MemoryScope,
) -> Result<MappedDocument, OpenClawError> {
    if document.path.trim().is_empty() || document.content.trim().is_empty() {
        return Err(OpenClawError::EmptyDocument);
    }
    let document_sha256 = sha256(&document.content);
    let sections = section_ranges(&document.content)
        .into_iter()
        .map(|section| mapped_section(document, config, scope, &document_sha256, section))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(MappedDocument {
        document_sha256,
        sections,
    })
}

fn section_ranges(content: &str) -> Vec<SectionRange> {
    let mut headings = Vec::<(String, usize)>::new();
    let mut current: Option<(String, usize)> = None;
    for (event, range) in Parser::new(content).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { .. }) => {
                current = Some((String::new(), range.start));
            }
            Event::Text(text) | Event::Code(text) if current.is_some() => {
                if let Some((title, _)) = current.as_mut() {
                    title.push_str(&text);
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some((title, start)) = current.take() {
                    headings.push((title, start));
                }
            }
            _ => {}
        }
    }
    if headings.is_empty() {
        return vec![SectionRange {
            heading: None,
            start: 0,
            end: content.len(),
        }];
    }
    let mut sections = Vec::with_capacity(headings.len() + 1);
    if headings[0].1 > 0 && !content[..headings[0].1].trim().is_empty() {
        sections.push(SectionRange {
            heading: None,
            start: 0,
            end: headings[0].1,
        });
    }
    for (index, (heading, start)) in headings.iter().enumerate() {
        sections.push(SectionRange {
            heading: Some(heading.clone()),
            start: *start,
            end: headings.get(index + 1).map_or(content.len(), |next| next.1),
        });
    }
    sections
}

fn mapped_section(
    document: &OpenClawDocument,
    config: &OpenClawConfig,
    scope: &MemoryScope,
    document_sha256: &str,
    section: SectionRange,
) -> Result<MappedSection, OpenClawError> {
    let line_start = line_at(&document.content, section.start);
    let line_end = line_at(&document.content, section.end.saturating_sub(1));
    let heading = section.heading;
    let fragment_kind = heading.as_deref().map_or(FragmentKind::Fact, |title| {
        let normalized = title.to_ascii_lowercase();
        if normalized.contains("preference") {
            FragmentKind::Preference
        } else if normalized.contains("constraint") {
            FragmentKind::Constraint
        } else {
            FragmentKind::Fact
        }
    });
    let source_id = format!(
        "openclaw/{}/{document_sha256}/{}/heading/{}/lines/{line_start}-{line_end}",
        config.source_version,
        document.path,
        heading.as_deref().unwrap_or("preamble")
    );
    let fragment_id = deterministic_fragment_id(&format!(
        "{source_id}\n{}\n{}",
        heading.as_deref().unwrap_or_default(),
        &document.content[section.start..section.end]
    ))?;
    let mut metadata = Metadata::new();
    metadata.insert(
        "source_path".to_owned(),
        Value::String(document.path.clone()),
    );
    metadata.insert(
        "content_sha256".to_owned(),
        Value::String(document_sha256.to_owned()),
    );
    metadata.insert(
        "heading".to_owned(),
        heading.clone().map_or(Value::Null, Value::String),
    );
    metadata.insert("line_start".to_owned(), json!(line_start));
    metadata.insert("line_end".to_owned(), json!(line_end));
    metadata.insert(
        "openclaw_source_version".to_owned(),
        Value::String(config.source_version.clone()),
    );
    Ok(MappedSection {
        heading,
        line_start,
        line_end,
        fragment: CognitiveFragment {
            id: fragment_id,
            scope: scope.clone(),
            session_id: None,
            trace_id: None,
            kind: fragment_kind,
            content: FragmentContent::Text {
                text: document.content[section.start..section.end].to_owned(),
            },
            source: FragmentSource {
                source_type: SourceType::Import,
                source_id,
                actor_id: None,
                observer_id: Some("jc-memory-adapter-openclaw".to_owned()),
                tool_id: None,
                uri: Some(format!("file://{}", document.path)),
            },
            perspective: Perspective::System,
            occurred_at: document.modified_at,
            observed_at: document.modified_at,
            confidence: 0.0,
            importance: config.default_importance,
            validity: ValidityStatus::Unverified,
            security_label: config.security_label,
            evidence: Vec::new(),
            metadata,
        },
    })
}

fn line_at(content: &str, offset: usize) -> usize {
    content[..offset.min(content.len())]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1
}

fn sha256(content: &str) -> String {
    hex::encode(Sha256::digest(content.as_bytes()))
}

fn deterministic_fragment_id(seed: &str) -> Result<FragmentId, OpenClawError> {
    let digest = Sha256::digest(seed.as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let value = format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    );
    Ok(FragmentId::from_str(&value)?)
}
