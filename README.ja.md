# core-rust

[English](README.md) | [日本語](README.ja.md)

`core-rust` は、CDISC 形式のルールと試験データを扱うための技術プレビュー版
バリデーションエンジンです。Rust 製 CLI、JSON/CSV/log レポート、P21PORT
変換支援、CDISC Open Rules 互換性検証ハーネスを含みます。

> ステータス: 技術プレビューです。このプロジェクトは独立した非公式実装であり、
> 公式 CDISC Validator ではありません。規制提出判断の唯一の根拠としては使用しないでください。

## 想定用途

次のような用途に向いています。

- SDTM/ADaM 風データに対する補助的なチェック
- JSON / YAML の CDISC CORE 風ルールの検証
- golden expected output との構造比較
- JSON / CSV / log 形式の結果確認
- P21 ルール export から P21PORT draft rule への変換調査
- CDISC Open Rules との互換性・差分・provenance の監査

公式 Validator の代替ではありません。提出・本番判断では、公式 Validator と
組織内の検証プロセスで必ず確認してください。

## ビルドと実行

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

## バリデーション実行例

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

## 対応入力

ルール:

- JSON
- YAML

データ:

- CSV
- DatasetPackageJson 風 JSON
- SAS XPT v5 subset

DatasetPackageJson の JavaScript safe integer range を超える数値は、暗黙の
精度低下を避けるため文字列として読み込まれる場合があります。

XPT 対応は境界を設けた v5 parser subset です。提出品質の XPORT
transport 妥当性確認は、公式ツールで行ってください。

メタデータ:

- Define-XML
- controlled terminology JSON
- 外部辞書 JSON / CSV

## 評価できる内容

現在の engine は、record-level / dataset-level checks、filters、derivations、
aggregate/group statistics、sorting、row numbers、joins、Match_Datasets 風 checks、
codelist checks、Define-XML metadata checks、小さな normalized expression subset
を扱えます。

USDM / Open Rules 対応には、対象を絞った hand-port check が含まれます。これらは
Open Rules provenance で別管理されており、一般的な JSONata 対応とは読まないでください。

## Open Rules 互換性の読み方

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

scheduled upstream workflow は、default scoreboard、strict scoreboard、
default-vs-strict delta artifact をアップロードします。

## P21PORT 支援

Python の `cdisc_rulekit` は、P21 rule export の棚卸し、変換候補分類、
draft rule 生成、実行、構造比較を支援します。

```sh
python -m pip install -e ".[test]"
PYTHONPATH=src python3 scripts/p21port_smoke.py --work-dir target/p21port-smoke
```

読み取り専用 pilot の例:

```sh
python -m cdisc_rulekit.cli pilot-preflight \
  --p21-rules input/p21/cdisc_rule_definitions_latest_2204.csv \
  --p21-domain-map input/p21/cdisc_rule_domain_map.csv \
  --open-rules-repo input/cdisc-open-rules-main.zip \
  --out output/reports \
  --standard SDTM-IG \
  --limit 20
```

生成物は review 用 draft です。明示的に export しない限り、Open Rules の
既存 `Published/` は変更されません。

## CLI の終了コード方針

`core-rs validate` は、validation 実行と report 生成が完了した場合、report 内に
failed / skipped rule result が含まれていても既定では exit `0` になります。これは、
手元確認で report を読む前にコマンド自体が失敗扱いになるのを避けるためです。

CI や release gate では、明示的な fail policy を使ってください。

```sh
core-rs validate ... --fail-on failed
core-rs validate ... --fail-on failed,skipped
core-rs validate ... --strict
```

`--strict` は failed と skipped の両方で失敗する設定と同等です。これらの mode の
non-zero exit は、report は生成されたが、指定した validation result policy を満たさなかった
ことを意味します。

## リリースと監査証跡

release artifact には provenance manifest を添付してください。

以下はローカル smoke 用の例です。review 済み release bundle では、この例の後に
示すより厳格な policy flags を使ってください。`--source-root .` が review 対象の
`Cargo.lock` を指すように、以下のコマンドはリポジトリ root から実行してください。

```sh
cargo build --release -p core-cli
mkdir -p target/release-provenance/bin
cp target/release/core-rs target/release-provenance/bin/core-rs
cargo run -p xtask -- release-manifest \
  --out target/release-provenance/release-manifest.json \
  --artifact-root target/release-provenance \
  --source-root . \
  --artifact target/release-provenance/bin/core-rs
cargo run -p xtask -- release-verify \
  --manifest target/release-provenance/release-manifest.json \
  --artifact-root target/release-provenance \
  --source-root . \
  --require-artifact \
  --require-cargo-lock
```

review 済み release bundle では、[Release reproducibility](docs/release-reproducibility.md)
にある厳格なコマンド例を使い、`--artifact`、`--artifact-root`、
`--source-root`、`--require-artifact`、`--require-cargo-lock`、verification
policy flags を指定してください。local smoke は artifact の存在と hash を確認し、
review 済み release verification ではさらに target triple、clean git provenance、
CI run metadata、`SOURCE_DATE_EPOCH` も要求します。

CI では host の `core-rs` バイナリを build し、SHA-256 を manifest に記録し、
verify したうえで manifest を GitHub Actions artifact として保存します。

関連ドキュメント:

- [Release reproducibility](docs/release-reproducibility.md)
- [Open Rules oracle harness](docs/open-rules-oracle-harness.md)
- [Open Rules upstream regression gate](docs/open-rules-upstream-regression-gate.md)
- [XPT fuzzing](docs/xpt-fuzzing.md)
- [Rust file split plan](docs/rust-file-split-plan.md)

## ワークスペース構成

- `apps/cli`: command-line interface
- `crates/core-api`: validation orchestration API
- `crates/core-rule-model`: rule parsing and normalization
- `crates/core-data`: dataset loading and dataset operations
- `crates/core-engine`: rule evaluation
- `crates/core-cdisc-library`: Define-XML, CT, dictionary parsing
- `crates/core-report`: JSON, CSV, log report writing
- `src/cdisc_rulekit`: Python P21/Open Rules conversion utilities
- `tests/fixtures`: golden and compatibility fixtures

## ライセンス

MIT License です。詳細は [LICENSE](LICENSE) を参照してください。

## 謝辞

このリポジトリでは相互運用性の説明のために CDISC、SDTM、ADaM、Define-XML
などの用語を使用しています。これらの名称は各権利者に帰属します。本プロジェクトは
独立した非公式実装です。
