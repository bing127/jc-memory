//! 版本化 Benchmark Fixture 的解析和标准 Case 转换。

use std::{collections::BTreeSet, fs, path::Path};

use chrono::{DateTime, Utc};
use jc_memory_activation::{RecallLimits, RecallQuery};
use jc_memory_protocol::{
    CognitiveFragment, FragmentContent, FragmentKind, FragmentSource, MemoryPacket, MemoryScope,
    Perspective, RecallMode, SecurityLabel, SessionId, SourceType, ValidityStatus,
};
use serde::Deserialize;
use thiserror::Error;

use crate::{BenchmarkCase, BenchmarkReport, CaseExecutor, ExpectedAnswer, RunnerError, run_cases};

const SCHEMA_VERSION: &str = "jcmem.suite.v1";
const FIXTURE_NAMESPACE: &str = "aaaaaaaa-aaaa-7aaa-8aaa-aaaaaaaaaaaa";

/// 经过校验的 Benchmark Fixture。
#[derive(Clone, Debug)]
pub struct Suite {
    /// 数据集稳定名称。
    pub dataset: String,
    /// 数据集或 Fixture 版本。
    pub version: String,
    /// 许可声明；私有 Fixture 使用 `PRIVATE-FIXTURE`。
    pub license: String,
    /// 原始来源和 Digest 元数据。
    pub source: SuiteSource,
    /// 是否只用于 Smoke 验证。
    pub smoke: bool,
    /// 保留原始 Case ID 的标准化输入。
    pub cases: Vec<SuiteCase>,
}

/// Fixture 来源描述。
#[derive(Clone, Debug)]
pub struct SuiteSource {
    /// 来源类型，例如 `repository_fixture`。
    pub kind: String,
    /// 固定来源 URI 或仓库内路径。
    pub uri: String,
    /// 版本化内容 Digest。
    pub sha256: String,
}

/// 一个 Fixture Case 的结构化内容。
#[derive(Clone, Debug)]
pub struct SuiteCase {
    /// 原始数据集 Case ID。
    pub id: String,
    /// 跨 Session 的 Fragment。
    pub sessions: Vec<SuiteSession>,
    /// 查询文本和模式。
    pub query: SuiteQuery,
    /// 不依赖语言模型措辞的预期信号。
    pub expected: SuiteExpected,
    /// 用例类别标签。
    pub tags: BTreeSet<String>,
}

/// 一个 Session 及其时间排序的轮次。
#[derive(Clone, Debug)]
pub struct SuiteSession {
    /// 稳定 Session ID。
    pub id: SessionId,
    /// Session 内的原始 Fragment。
    pub turns: Vec<SuiteTurn>,
}

/// 一个最小原始文本轮次。
#[derive(Clone, Debug)]
pub struct SuiteTurn {
    /// 稳定 Fragment ID。
    pub fragment_id: jc_memory_protocol::FragmentId,
    /// 原始文本。
    pub text: String,
    /// 发生时间。
    pub occurred_at: DateTime<Utc>,
}

/// Fixture 查询描述。
#[derive(Clone, Debug)]
pub struct SuiteQuery {
    /// 查询文本。
    pub text: String,
    /// Recall 模式。
    pub mode: RecallMode,
    /// 可选历史时间点。
    pub at_time: Option<DateTime<Utc>>,
}

/// Fixture 的结构化答案预期。
#[derive(Clone, Debug, Default)]
pub struct SuiteExpected {
    /// 预期 Point ID。
    pub point_ids: BTreeSet<jc_memory_protocol::PointId>,
    /// 预期 Evidence ID。
    pub evidence_ids: BTreeSet<jc_memory_protocol::EvidenceId>,
    /// 是否预期拒答。
    pub abstention: bool,
}

/// Fixture 解析、校验或转换失败。
#[derive(Debug, Error)]
pub enum SuiteError {
    /// 文件读取失败。
    #[error("读取 Benchmark Fixture 失败: {0}")]
    Io(#[from] std::io::Error),
    /// JSON 结构无效。
    #[error("Benchmark Fixture JSON 无效: {0}")]
    Json(#[from] serde_json::Error),
    /// Fixture 不满足稳定契约。
    #[error("Benchmark Fixture 契约无效: {0}")]
    Invalid(String),
    /// UUID 标识无效。
    #[error("Benchmark Fixture 标识无效: {0}")]
    Id(#[from] jc_memory_protocol::IdParseError),
    /// 时间戳无效。
    #[error("Benchmark Fixture 时间无效: {0}")]
    Time(#[from] chrono::ParseError),
}

/// Fixture Runner 执行失败。
#[derive(Debug, Error)]
pub enum SuiteRunError {
    /// Fixture 转换失败。
    #[error(transparent)]
    Fixture(#[from] SuiteError),
    /// 注入的 Case 执行器失败。
    #[error(transparent)]
    Runner(#[from] RunnerError),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSuite {
    schema_version: String,
    dataset: String,
    version: String,
    license: String,
    source: RawSource,
    smoke: bool,
    cases: Vec<RawCase>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSource {
    kind: String,
    uri: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCase {
    id: String,
    sessions: Vec<RawSession>,
    query: RawQuery,
    expected: RawExpected,
    tags: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSession {
    id: String,
    turns: Vec<RawTurn>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTurn {
    fragment_id: String,
    text: String,
    occurred_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawQuery {
    text: String,
    mode: String,
    at_time: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawExpected {
    point_ids: Vec<String>,
    evidence_ids: Vec<String>,
    abstention: bool,
}

/// 读取并校验一个版本化 Fixture。
pub fn load_suite<P: AsRef<Path>>(path: P, expected_dataset: &str) -> Result<Suite, SuiteError> {
    let raw: RawSuite = serde_json::from_slice(&fs::read(path)?)?;
    if raw.schema_version != SCHEMA_VERSION {
        return Err(SuiteError::Invalid(format!(
            "schema_version 必须是 {SCHEMA_VERSION}"
        )));
    }
    if raw.dataset != expected_dataset {
        return Err(SuiteError::Invalid(format!(
            "数据集必须是 {expected_dataset}"
        )));
    }
    if raw.version.trim().is_empty()
        || raw.license.trim().is_empty()
        || raw.source.kind.trim().is_empty()
        || raw.source.uri.trim().is_empty()
        || raw.source.sha256.trim().is_empty()
        || raw.cases.is_empty()
    {
        return Err(SuiteError::Invalid(
            "版本、许可、来源、Digest 和 Case 不能为空".to_owned(),
        ));
    }
    let mut case_ids = BTreeSet::new();
    if raw
        .cases
        .iter()
        .any(|case| !case_ids.insert(case.id.as_str()))
    {
        return Err(SuiteError::Invalid("Case ID 不能重复".to_owned()));
    }
    let cases = raw
        .cases
        .into_iter()
        .map(convert_case)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Suite {
        dataset: raw.dataset,
        version: raw.version,
        license: raw.license,
        source: SuiteSource {
            kind: raw.source.kind,
            uri: raw.source.uri,
            sha256: raw.source.sha256,
        },
        smoke: raw.smoke,
        cases,
    })
}

impl Suite {
    /// 将所有 Fixture Case 转换为统一 Harness Case。
    ///
    /// # Errors
    ///
    /// 当 Fixture 中的 UUID 或时间戳不能转换为协议类型时返回错误。
    pub fn to_benchmark_cases(&self) -> Result<Vec<BenchmarkCase>, SuiteError> {
        self.cases.iter().map(to_benchmark_case).collect()
    }

    /// 使用注入执行器运行整套 Fixture 并生成统一报告。
    ///
    /// # Errors
    ///
    /// 当 Fixture 转换或某个 Case 执行失败时返回错误。
    pub fn run<E: CaseExecutor>(&self, executor: &E) -> Result<BenchmarkReport, SuiteRunError> {
        let cases = self.to_benchmark_cases()?;
        let results = run_cases(&cases, executor)?;
        Ok(BenchmarkReport::build(&cases, &results))
    }
}

/// 只依据 Packet 的结构化缺证信号判断拒答。
#[must_use]
pub fn is_abstention(packet: &MemoryPacket) -> bool {
    packet.points.is_empty() && !packet.missing_evidence.is_empty()
}

fn convert_case(raw: RawCase) -> Result<SuiteCase, SuiteError> {
    if raw.id.trim().is_empty() || raw.sessions.is_empty() {
        return Err(SuiteError::Invalid(
            "Case ID 和 Session 不能为空".to_owned(),
        ));
    }
    let sessions = raw
        .sessions
        .into_iter()
        .map(|session| {
            let id = session.id.parse()?;
            let turns = session
                .turns
                .into_iter()
                .map(|turn| {
                    Ok(SuiteTurn {
                        fragment_id: turn.fragment_id.parse()?,
                        text: turn.text,
                        occurred_at: turn.occurred_at.parse()?,
                    })
                })
                .collect::<Result<Vec<_>, SuiteError>>()?;
            Ok(SuiteSession { id, turns })
        })
        .collect::<Result<Vec<_>, SuiteError>>()?;
    let mode = parse_mode(&raw.query.mode)?;
    let at_time = raw.query.at_time.map(|time| time.parse()).transpose()?;
    let point_ids = raw
        .expected
        .point_ids
        .into_iter()
        .map(|id| id.parse())
        .collect::<Result<BTreeSet<_>, _>>()?;
    let evidence_ids = raw
        .expected
        .evidence_ids
        .into_iter()
        .map(|id| id.parse())
        .collect::<Result<BTreeSet<_>, _>>()?;
    Ok(SuiteCase {
        id: raw.id,
        sessions,
        query: SuiteQuery {
            text: raw.query.text,
            mode,
            at_time,
        },
        expected: SuiteExpected {
            point_ids,
            evidence_ids,
            abstention: raw.expected.abstention,
        },
        tags: raw.tags.into_iter().collect(),
    })
}

fn to_benchmark_case(case: &SuiteCase) -> Result<BenchmarkCase, SuiteError> {
    let namespace_id = FIXTURE_NAMESPACE.parse()?;
    let scope = MemoryScope::new(namespace_id);
    let setup = case
        .sessions
        .iter()
        .flat_map(|session| {
            session.turns.iter().map(|turn| CognitiveFragment {
                id: turn.fragment_id,
                scope: scope.clone(),
                session_id: Some(session.id),
                trace_id: None,
                kind: FragmentKind::Observation,
                content: FragmentContent::Text {
                    text: turn.text.clone(),
                },
                source: FragmentSource {
                    source_type: SourceType::Import,
                    source_id: "versioned-fixture".to_owned(),
                    actor_id: None,
                    observer_id: None,
                    tool_id: None,
                    uri: Some("repository://benchmark-fixture".to_owned()),
                },
                perspective: Perspective::ThirdPerson,
                occurred_at: turn.occurred_at,
                observed_at: turn.occurred_at,
                confidence: 1.0,
                importance: 0.5,
                validity: ValidityStatus::Verified,
                security_label: SecurityLabel::Internal,
                evidence: Vec::new(),
                metadata: serde_json::Map::new(),
            })
        })
        .collect();
    Ok(BenchmarkCase {
        id: case.id.clone(),
        setup,
        query: RecallQuery {
            scope,
            mode: case.query.mode,
            text: case.query.text.clone(),
            entities: Vec::new(),
            at_time: case.query.at_time,
            limits: RecallLimits {
                max_seeds: 32,
                max_neighbors: 32,
                max_hops: 4,
                max_nodes: 64,
                timeout_ms: 500,
            },
        },
        expected: ExpectedAnswer {
            point_ids: case.expected.point_ids.clone(),
            evidence_ids: case.expected.evidence_ids.clone(),
            abstention_expected: Some(case.expected.abstention),
            ..ExpectedAnswer::default()
        },
        tags: case.tags.clone(),
    })
}

fn parse_mode(mode: &str) -> Result<RecallMode, SuiteError> {
    match mode {
        "exact" => Ok(RecallMode::Exact),
        "semantic" => Ok(RecallMode::Semantic),
        "temporal" => Ok(RecallMode::Temporal),
        "causal" => Ok(RecallMode::Causal),
        "entity" => Ok(RecallMode::Entity),
        "current_state" => Ok(RecallMode::CurrentState),
        "historical_state" => Ok(RecallMode::HistoricalState),
        _ => Err(SuiteError::Invalid(format!("不支持的 Recall mode: {mode}"))),
    }
}
