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
(`generic_engine` / `rule_specific_engine_semantics` / `compatibility_policy` /
`rule_id_hand_port`) が入ります。CSV は既存ツール連携のため、
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

Python の `cdisc_rulekit` は、利用者が正規に入手・利用許諾を持つ
P21-style rule catalog CSV の棚卸し、変換候補分類、draft rule 生成、
実行、構造比較を支援します。

P21PORT は Pinnacle 21 の proprietary rule definition を取得、scrape、
export する機能ではありません。特に、Pinnacle 21 Community から
`--p21-rules` に渡せる rule definition CSV が出力できる前提にはしていません。
利用許諾上問題なく使える rule catalog だけを持ち込んでください。

`p21-community/configs` などの公開 Pinnacle 21 Community configuration source を
使う場合も、事前に適用ライセンスを確認してください。生成した catalog や
adapted rule は、ライセンス上共有が許される場合を除き、ローカル/利用者持ち込み
artifact として扱います。

利用許諾上処理できるローカル XML configuration file がある場合は、
`build-readonly` の前に XML-to-catalog converter を使えます。

```sh
PYTHONPATH=src python3 -m cdisc_rulekit.cli convert-p21-config \
  --input /path/to/local/p21-config.xml \
  --source-label sdtm33 \
  --out target/p21-config-catalog

PYTHONPATH=src python3 -m cdisc_rulekit.cli build-readonly \
  --p21-rules target/p21-config-catalog/p21_rules_normalized.csv \
  --open-rules-repo input/cdisc-open-rules-main.zip \
  --out output/reports
```

converter は `p21_rules_normalized.csv`、`p21_rules_normalized.jsonl`、
`extraction_report.md` を出力します。これは local review 用の best-effort extractor
であり、Pinnacle 21 configuration schema の完全変換器ではありません。
configuration file の download は行わず、生成 catalog もライセンス上許される場合を
除き commit / 共有しないでください。P21PORT catalog として使う前に、生成された
CSV/JSONL を review してください。
長期 catalog 比較で path に依存しない source identifier が必要な場合は
`--source-label` を指定してください。release や継続比較の workflow では、
default の入力順 label ではなく明示 label の利用を推奨します。

XML parsing は install 済み環境では `defusedxml` を使います。optional dependency を
入れる前の source-tree smoke では Python 標準 library parser へ fallback する場合が
ありますが、その場合も parse 前に DTD/entity declaration を拒否し、malformed /
unreadable XML は安定した `error: ...` 形式の CLI message として報告します。
同名ファイルの識別のため、terminal error には full local input path が含まれる場合が
あります。外部へログを共有する前に stderr の local path を sanitize してください。

```sh
python -m pip install -e ".[test]"
PYTHONPATH=src python3 scripts/p21port_smoke.py --work-dir target/p21port-smoke
# core-rs build 後は、生成 fixture を実 engine でも検証できます:
PYTHONPATH=src python3 scripts/p21port_smoke.py \
  --work-dir target/p21port-real-smoke \
  --real-engine-command 'target/debug/core-rs validate'
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

生成物は review 用 draft であり、Pinnacle 21 の代替ではありません。
明示的に export しない限り、Open Rules の既存 `Published/` は変更されません。

`run-core` は、dry-run も含め、毎回**新しい** `--out` ディレクトリを必要とします。
実行時は `reports/core_run_evidence.json` に、入力・期待値ファイルと起動した実行ファイルの
SHA-256、実行コマンド・設定、実行結果、出力のハッシュを保存します。
実行後にも入力を確認し、変更やレポート未生成を検出した場合は失敗します。
中断した記録は `started` のままで、完了扱いにはなりません。

この記録は**実行の証跡**であり、期待値との一致や人による承認を意味しません。
Python や Cargo 経由の場合、記録する実行ファイルは起動元だけで、全依存や生成後の
エンジン本体は網羅しません。実用検証では、固定して release-verify で検証した
バイナリの直接指定を推奨します。比較・承認・保存を分ける手順は
[ルール検証パイロットのチェックリスト](docs/rule-validation-pilot.md)を参照してください。

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

`--strict` は failed / skipped に加え、フィルターですべてのルールを除外した場合など、
結果が0件のときも失敗します。`--fail-on` は指定した status のみを検査します。
結果に対する終了コード判定の前に report を書きますが、入力・読み込み・出力のエラーでは
report 生成前に失敗する場合があります。non-zero exit を「問題なし」と扱わないでください。

### ルール入力と出力先の安全性

- ルールファイルの読み込みが0件の場合は、`--strict` なしでもエラーになります。
  `--local-rules` はディレクトリ直下の JSON/YAML のみを読み、サブディレクトリは探索しません。
  upstream の階層化された `Published/` をそのまま指定しても全ルールは読み込まれません。
  ファイルを明示するか、公式ケース群の検証には Open Rules harness を使ってください。
- 読み込むルールIDは一意でなければなりません。同一定義の二重指定や、ディレクトリと
  ファイルの重複指定もエラーです。競合時は両方の入力パスを示し、`--rules` /
  `--exclude-rules` で絞り込む前に停止します。入力順で優先順位は決まりません。
  `--rules` の選択リスト内で同じIDを繰り返した場合は、従来どおり1回だけ選択します。
- Open Rules の candidate 実行、P21PORT の実エンジン実行も含め、実行ごとに新しい出力先を
  指定してください。既存の `report.json` / `report.csv` / `validation.log` がある場合は、
  今回出力しない形式であっても書き込みを拒否します。既存レポートや無関係なファイルは
  bundle writer が削除・上書きしません。一時的な `.core-rs-report.lock` で同時書き込みも
  防ぎます。異常終了で lock が残った場合は新しい出力先を使い、書き込み失敗時の途中ファイルを
  完了した検証結果として扱わないでください。

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
