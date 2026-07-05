# core-rust

`core-rust` is a technical-preview validation engine for CDISC-style rules and
study data. It provides a Rust CLI, report writers, and compatibility harnesses
for supplemental validation, regression testing, and rule-conversion research.

> Status: technical preview. This project is independent and unofficial. It is
> not the official CDISC Validator, not endorsed by CDISC, and not a sole
> authority for regulatory submission decisions.

## English

### Who This Is For

Use `core-rust` when you need to:

- run supplemental checks over SDTM/ADaM-like datasets
- test CDISC CORE-like rules in JSON or YAML
- compare candidate behavior with reviewed golden outputs
- inspect machine-readable JSON/CSV/log validation reports
- research P21PORT and CDISC Open Rules conversion workflows
- audit Open Rules compatibility with provenance-aware scoreboards

Do not use it as a drop-in replacement for the official CDISC Validator.
Submission-critical workflows should still be checked with the official
validator and your governed validation process.

### Install And Build

Requirements:

- Rust 1.93 or newer
- Python 3.11 or newer for the optional `cdisc_rulekit` utilities.
  CI currently tests Python 3.13.

```sh
cargo check --workspace --locked
cargo test --workspace --locked
cargo build --release -p core-cli
```

The CLI binary is named `core-rs`.

```sh
cargo run -p core-cli -- validate --help
```

### Run A Validation

```sh
cargo run -p core-cli -- validate \
  --local-rules tests/fixtures/rules/regulatory \
  --dataset-path tests/fixtures/datasets/regulatory/study_package.json \
  --define-xml tests/fixtures/cdisc/regulatory_define.xml \
  --ct tests/fixtures/cdisc/regulatory_ct.json \
  --external-dictionary tests/fixtures/cdisc/regulatory_external_dictionary.csv \
  --log-level info \
  --output target/core-rust-report
```

Outputs:

```text
target/core-rust-report/report.json
target/core-rust-report/report.csv
target/core-rust-report/validation.log
```

`report.json` includes `execution_provenance` when the engine can classify the
rule path as `native_engine` or `rule_id_hand_port`. CSV keeps a stable
issue-row schema for downstream tools.

### Supported Inputs

Rules:

- JSON
- YAML

Data:

- CSV
- DatasetPackageJson-style JSON
- SAS XPT v5 subset

Metadata:

- Define-XML datasets, variables, codelists, value lists, where clauses,
  methods, comments, and documents
- controlled terminology JSON
- external dictionaries from JSON or CSV

### What The Engine Can Evaluate

The current engine supports record-level and dataset-level checks, filters,
derivations, aggregate/group statistics, sorting, row numbers, joins,
Match_Datasets-style checks, codelist checks, Define-XML metadata checks, and a
small normalized expression subset.

USDM/Open Rules support includes targeted hand-ported checks. These are tracked
separately in Open Rules provenance and should not be read as general JSONata
support.

### Open Rules Compatibility

The repository includes a CDISC Open Rules oracle harness. Scoreboards separate:

- supported matches and mismatches
- deferred oracle/fixture gaps
- no-official-oracle cases
- skipped unsupported cases
- native engine vs rule-id hand-port coverage
- strict identity scoring vs compatibility normalization

`supported_accuracy = 100%` means no mismatch inside the reviewed supported
denominator. It does not mean the full upstream corpus is implemented or that
the tool is regulatory-ready.

For audit runs:

```sh
cargo run -p xtask -- open-rules score --strict-scoring --help
cargo run -p xtask -- open-rules score-delta --help
```

The scheduled upstream workflow uploads default scoreboards, strict scoreboards,
and default-vs-strict delta artifacts.

### P21PORT Rulekit

The optional Python `cdisc_rulekit` package helps inspect P21 rule exports,
classify conversion candidates, generate draft P21PORT rules, run candidate
rules, and compare structural outputs.

```sh
python -m pip install -e ".[test]"
PYTHONPATH=src python3 scripts/p21port_smoke.py --work-dir target/p21port-smoke
```

Typical read-only pilot:

```sh
python -m cdisc_rulekit.cli pilot-preflight \
  --p21-rules input/p21/cdisc_rule_definitions_latest_2204.csv \
  --p21-domain-map input/p21/cdisc_rule_domain_map.csv \
  --open-rules-repo input/cdisc-open-rules-main.zip \
  --out output/reports \
  --standard SDTM-IG \
  --limit 20
```

P21PORT outputs are draft/review artifacts. Existing Open Rules `Published/`
content is not modified unless you explicitly export into a target tree.

### Release And Audit Artifacts

Release artifacts should be accompanied by a provenance manifest:

```sh
cargo run -p xtask -- release-manifest --out target/release-provenance/release-manifest.json
cargo run -p xtask -- release-verify --manifest target/release-provenance/release-manifest.json
```

For reviewed release bundles, use the stricter command in
[Release reproducibility](docs/release-reproducibility.md) with
`--artifact`, `--artifact-root`, `--source-root`, and verification policy flags.

The CI release provenance gate builds the host `core-rs` binary, records its
SHA-256 in `release-manifest.json`, verifies the manifest, and uploads the
manifest as a GitHub Actions artifact.

See:

- [Release reproducibility](docs/release-reproducibility.md)
- [Open Rules oracle harness](docs/open-rules-oracle-harness.md)
- [Open Rules upstream regression gate](docs/open-rules-upstream-regression-gate.md)
- [XPT fuzzing](docs/xpt-fuzzing.md)
- [Rust file split plan](docs/rust-file-split-plan.md)

### Workspace Layout

- `apps/cli`: command-line interface
- `crates/core-api`: validation orchestration API
- `crates/core-rule-model`: rule parsing and normalization
- `crates/core-data`: dataset loading and dataset operations
- `crates/core-engine`: rule evaluation
- `crates/core-cdisc-library`: Define-XML, CT, and dictionary parsing
- `crates/core-report`: JSON, CSV, and log report writing
- `src/cdisc_rulekit`: Python P21/Open Rules conversion utilities
- `tests/fixtures`: golden and compatibility fixtures

## 日本語

`core-rust` は、CDISC 形式のルールと試験データを扱うための技術プレビュー版
バリデーションエンジンです。Rust 製 CLI、JSON/CSV/log レポート、P21PORT
変換支援、CDISC Open Rules 互換性検証ハーネスを含みます。

> ステータス: 技術プレビューです。このプロジェクトは独立した非公式実装であり、
> 公式 CDISC Validator ではありません。規制提出判断の唯一の根拠としては使用しないでください。

### 想定用途

次のような用途に向いています。

- SDTM/ADaM 風データに対する補助的なチェック
- JSON / YAML の CDISC CORE 風ルールの検証
- golden expected output との構造比較
- JSON / CSV / log 形式の結果確認
- P21 ルール export から P21PORT draft rule への変換調査
- CDISC Open Rules との互換性・差分・provenance の監査

公式 Validator の代替ではありません。提出・本番判断では、公式 Validator と
組織内の検証プロセスで必ず確認してください。

### ビルドと実行

必要なもの:

- Rust 1.93 以上
- Python 3.11 以上 (`cdisc_rulekit` を使う場合)。CI では Python 3.13
  を使用しています。

```sh
cargo check --workspace --locked
cargo test --workspace --locked
cargo build --release -p core-cli
```

CLI バイナリ名は `core-rs` です。

```sh
cargo run -p core-cli -- validate --help
```

### バリデーション実行例

```sh
cargo run -p core-cli -- validate \
  --local-rules tests/fixtures/rules/regulatory \
  --dataset-path tests/fixtures/datasets/regulatory/study_package.json \
  --define-xml tests/fixtures/cdisc/regulatory_define.xml \
  --ct tests/fixtures/cdisc/regulatory_ct.json \
  --external-dictionary tests/fixtures/cdisc/regulatory_external_dictionary.csv \
  --log-level info \
  --output target/core-rust-report
```

出力:

```text
target/core-rust-report/report.json
target/core-rust-report/report.csv
target/core-rust-report/validation.log
```

`report.json` には、判定できる場合に `execution_provenance`
(`native_engine` / `rule_id_hand_port`) が入ります。CSV は既存ツール連携のため、
安定した issue-row schema を維持します。

### 対応入力

ルール:

- JSON
- YAML

データ:

- CSV
- DatasetPackageJson 風 JSON
- SAS XPT v5 subset

メタデータ:

- Define-XML
- controlled terminology JSON
- 外部辞書 JSON / CSV

### Open Rules 互換性の読み方

Open Rules harness は、単純な pass 件数ではなく、以下を分けて集計します。

- supported match / mismatch
- deferred oracle / fixture gap
- official oracle が存在しない case
- unsupported skip
- native engine coverage
- rule-id hand-port coverage
- strict scoring と compatibility normalization の差分

`supported_accuracy = 100%` は、review 済み supported denominator 内で mismatch が
0 という意味です。全 upstream corpus を完全実装した、または規制用途で妥当、
という意味ではありません。

監査用には strict scoring と delta を確認してください。

```sh
cargo run -p xtask -- open-rules score --strict-scoring --help
cargo run -p xtask -- open-rules score-delta --help
```

### P21PORT 支援

Python の `cdisc_rulekit` は、P21 rule export の棚卸し、変換候補分類、
draft rule 生成、実行、構造比較を支援します。

```sh
python -m pip install -e ".[test]"
PYTHONPATH=src python3 scripts/p21port_smoke.py --work-dir target/p21port-smoke
```

生成物は review 用 draft です。明示的に export しない限り、Open Rules の
既存 `Published/` は変更されません。

### リリースと監査証跡

release artifact には provenance manifest を添付してください。

```sh
cargo run -p xtask -- release-manifest --out target/release-provenance/release-manifest.json
cargo run -p xtask -- release-verify --manifest target/release-provenance/release-manifest.json
```

review 済み release bundle では、[Release reproducibility](docs/release-reproducibility.md)
にある厳格なコマンド例を使い、`--artifact`、`--artifact-root`、
`--source-root`、verification policy flags を指定してください。

CI では host の `core-rs` バイナリを build し、SHA-256 を manifest に記録し、
verify したうえで manifest を GitHub Actions artifact として保存します。

### ライセンス

MIT License です。詳細は [LICENSE](LICENSE) を参照してください。

### 謝辞

このリポジトリでは相互運用性の説明のために CDISC、SDTM、ADaM、Define-XML
などの用語を使用しています。これらの名称は各権利者に帰属します。本プロジェクトは
独立した非公式実装です。
