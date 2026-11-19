# AE・DMの5ルール事前チェックを試す

同梱の公開サンプルで、RからXPTを出力した後の事前チェックを試します。対象はSDTMIG 3.4のAE・DMに対する5ルールです。提出用の正式なバリデーションではありません。

**対象環境:** Apple silicon Macと、arm64版のR。既存の試行ではR 4.6.1を使用しました。Rがなければ、所属先の方針に沿って[CRANのmacOS用ページ](https://cran.r-project.org/bin/macosx/)からApple silicon版を入れてください。パッケージのインストールにはインターネット接続が必要です。Git、Rust、データベース、公式COREは不要です。

## 1. セットアップ時間の計測を始める

アーカイブを受け取った後、開封やRのインストールを始める前に、時刻を控えます。下の「Rの準備」が終わるまでの経過時間を、分単位で記録してください。途中で止まった場合は、そこまでの時間と止まった手順を記録します。

アーカイブをクラウド同期や共有の対象になっていない、新しいローカル作業フォルダに置きます。ターミナルで、そのフォルダへ移動してください。ここからは同じターミナルで実行します。

## 2. アーカイブのハッシュを照合して展開する

```sh
shasum -a 256 core-rs-ae-dm-pilot-macos-arm64.tar.gz
```

表示されたSHA-256を、配布者から確認できる以下の値と照合してください。値が一致しない、または配布元を確認できない場合は、展開・実行せずに止めます。

```text
ab28b3d8c4a6466384e6551f17cb26dfba9539b5ce6b711b4f01bc5281cef4b9
```

一致したら展開します。

```sh
tar -xzf core-rs-ae-dm-pilot-macos-arm64.tar.gz
```

## 3. Rの準備をする

Rのバージョンとアーキテクチャを確認します。`aarch64`（または`arm64`）でなければ、この配布版の実行は見送り、理由を記録してください。

```sh
Rscript --vanilla -e 'cat(R.version.string, R.version$arch, "", sep = "\n")'
```

今回専用のRライブラリを作り、必要な4パッケージを入れます。

```sh
mkdir r-pilot-library
unset R_LIBS
export R_LIBS_USER="$PWD/r-pilot-library"
export R_LIBS_SITE="$R_LIBS_USER"
Rscript --vanilla -e 'install.packages(c("haven", "jsonlite", "digest", "processx"), lib = Sys.getenv("R_LIBS_USER"), repos = "https://cran.r-project.org", type = "binary")'
Rscript --vanilla -e 'p <- c("haven", "jsonlite", "digest", "processx"); lib <- Sys.getenv("R_LIBS_USER"); stopifnot(all(p %in% rownames(installed.packages(lib.loc = lib))), all(vapply(p, requireNamespace, logical(1), quietly = TRUE))); cat("R packages ready\n")'
```

`R packages ready` が出たら計測を止め、セットアップにかかった分数を記録します。Rを新規インストールしたかも記録してください。パッケージの取得に失敗した場合は、許可されていない通信や実行制限を回避せず、つまずいた手順を記録します。

## 4. 実行結果を4項目に分けて記録する

同梱のR実行例を動かします。自分の業務データは使いません。スクリプトが公開サンプルからXPTを書き出し、正常例、異常例、必須列欠落、修正後を確認します。

```sh
BUNDLE="$PWD/core-rs-ae-dm-pilot-macos-arm64"
Rscript --vanilla "$BUNDLE/example.R" "$BUNDLE" "$PWD/pilot-project"
echo "exit status: $?"
```

`pilot-project` は実行例が新規作成するので、事前に作らないでください。再試行時は `pilot-project-retry` など別名を指定します。

実行例は全項目が完了したときに4行を表示します。終了コードが `0` であることと、以下の結果を確認してください。

| 確認項目 | 期待する表示と結果 |
|---|---|
| 正常 | `Normal: 5 rules checked, 0 findings.` 5ルールを実行し、指摘0件。 |
| 異常 | `Abnormal: 5 controls, 5 rules checked per control, 1 matching finding per control.` 1セルだけ変えた5例で、各例とも5ルールを実行。期待したルール、レコード、変数に指摘が各1件。 |
| 修正後 | `Repaired: 5 rules checked, 0 findings.` Rで値を元に戻してXPTを再出力し、5ルールを実行して指摘0件。 |
| 未評価 | `Missing column: not_evaluated (input).` 必須列がないので評価できず、指摘件数は未算出。指摘0件ではありません。 |

途中で止まった場合は、確認できた結果だけを記録します。表示されなかった項目は「未実施」とし、成功として扱いません。

## 5. 結果記録だけを返す

[結果記録テンプレート](AE-DM-パイロット結果記録.md)に、4項目の結果、所要時間、つまずいた点を書いてください。XPT、レポート、詳細ログ、スクリーンショット、被験者ID、セルの値、端末名、絶対パスは送らないでください。原因を調べる場合も、作業フォルダ内の出力はローカルでだけ確認し、返信には止まった手順と状況の要約だけを書きます。
