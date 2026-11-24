# 参与 PANIM

[English](CONTRIBUTING.md) | 简体中文

本文件是 `CONTRIBUTING.md` 的中文版。如果两个文件的内容不一致，以英文版 `CONTRIBUTING.md` 为准。

本文件写给修改 PANIM 的人：它说明如何构建和测试 PANIM、修改需要遵守的规则，以及修改如何进入 PANIM。`README.zh-CN.md` 写给使用 PANIM 的人。

修改 PANIM 之前，请阅读这些文档（英文）：

- `docs/DESIGN.md` 说明 PANIM 做什么。
- `docs/ARCHITECTURE.md` 说明我们如何构建 PANIM。
- `docs/ROADMAP.md` 列出里程碑、当前状态和下一步工作。

有些文档会引用 `DISCUSSION.md` 的章节。这个文件是决策记录，只存在于开发仓库中。

## 两个仓库

- **公开仓库** `SakuraRainn/panim` 只包含发布版本。每个发布版本是 `main` 上的一个提交和一个标签。这个提交与该版本签名的源代码归档完全相同。
- **开发仓库**是私有的。维护者在那里工作。工作的历史只在那里。

因此，公开仓库不能合并 pull request（见“修改如何进入 PANIM”一节）。

## 构建和测试

在 Linux 上可以用两种方式构建 PANIM：

- **用 Nix。** flake 提供所有工具和库。维护者用这种构建进行开发。Nix 用户可以从 flake 安装 PANIM。
- **不用 Nix。** 库来自你的系统，Rust 来自 rustup，一个脚本构建 PANIM 版 TDLib。发布的安装包来自这种构建，在容器中进行（`docker/check.sh`）。

两种方式使用同一个 TDLib 补丁 `nix/tdlib-no-direct.patch`。没有这个补丁，TDLib 可能建立不经过身份代理的连接。

### 用 Nix 构建

flake 提供 Rust 工具链、WebKitGTK、Node.js、pnpm、PANIM 的 TDLib 和其他库。不需要 rustup。所有步骤都在 `nix develop` 的 shell 中进行。

1. 在仓库根目录运行 `nix develop`。第一次运行时，Nix 会构建带补丁的 PANIM 版 TDLib。这大约需要 10 分钟或更长时间。
2. 在 `crates/app` 中运行 `pnpm install`。
3. 在 `crates/app` 中运行 `pnpm build`。Rust crate `panim` 编译之前需要 `crates/app/dist`。
4. 在根目录运行 `cargo test`、`cargo clippy --all-targets` 和 `cargo fmt --check`。
5. 在 `crates/app` 中运行 `pnpm check`。如果某种语言缺少文本，它会失败。
6. 要以开发模式启动应用，在 `crates/app` 中运行 `pnpm tauri dev`。
7. 要构建软件包，在根目录运行 `nix build`。结果是 `result/bin/panim`。

### 不用 Nix 构建

以下步骤适用于 Ubuntu 24.04。在其他系统上，用它的包管理器安装相同的库。不要在 `nix develop` 的 shell 中进行这些步骤。

1. 安装系统软件包：

   ```
   sudo apt install build-essential cmake gperf curl file pkg-config libssl-dev zlib1g-dev libsqlite3-dev \
     libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev librsvg2-dev libxdo-dev libayatana-appindicator3-dev
   ```

2. 安装 rustup（https://rustup.rs）。在仓库中，rustup 使用 `rust-toolchain.toml` 中的 Rust 版本。
3. 在仓库中运行 `rustup component add clippy rustfmt`。
4. 安装 Node.js 24 和 pnpm。
5. 在根目录运行 `scripts/build-tdlib.sh`。这大约需要 10 分钟或更长时间。
   - 脚本下载 TDLib 的源代码，并检查它的 SHA-256。
   - 它应用 `nix/tdlib-no-direct.patch`，并在 `target/tdlib` 中构建 `libtdjson`。
6. 告诉 pkg-config TDLib 的位置。每个新的 shell 都要做一次：

   ```
   export PKG_CONFIG_PATH="$PWD/target/tdlib/lib/pkgconfig"
   ```

7. 进行“用 Nix 构建”的第 2 到 6 步。
8. 要构建软件包，在根目录运行 `scripts/build-packages.sh`。结果是 `target/release/bundle` 中的 `.deb`、`.rpm` 和 AppImage。

`cargo build` 构建的程序在 `target/tdlib/lib` 中找到 `libtdjson`。`.deb` 和 `.rpm` 把 `libtdjson` 放到 `/usr/lib/panim`，程序在那里找到它。AppImage 把 `libtdjson` 放在它的 `usr/lib` 中。

`docker/check.sh` 在一个干净的 Ubuntu 24.04 容器中进行以上所有步骤。然后它在 Ubuntu 中安装 `.deb`，在 Fedora 中安装 `.rpm`，并在 Xvfb 中启动 PANIM。它需要 Docker 或 rootless Podman（`CONTAINER_ENGINE`）。`TDLIB_JOBS` 设置 TDLib 构建的任务数。每个任务大约需要 2 GB 内存。输出在 `target/container` 中。

这两个脚本也包含 macOS 的步骤。只有 GitHub 的 macOS 运行器运行过它们（`.github/workflows/macos-check.yml` 和 `macos-release.yml`）。还没有人在 Mac 上测试过。

### 测试数据和测试

把 `PANIM_DATA` 设为一个测试文件夹。这样，测试或开发启动不会使用你在 `~/.local/share/panim` 中的真实数据。

测试不使用网络，也不使用任何账户：

- `panim-github` 的测试使用假的 HTTP 客户端。
- `panim-telegram` 和应用的测试使用假的 TDLib。永远不要在测试中登录 Telegram。

维护者在每次发布之前运行全部测试。在开发仓库中，每次修改也会自动运行测试。

### 构建中的 Telegram 应用参数

Telegram 通过应用参数识别每个应用：一个 `api_id` 和一个 `api_hash`。PANIM 的源代码中永远没有 PANIM 的参数。

- `panim-telegram` 的构建从 `PANIM_TELEGRAM_API_ID` 和 `PANIM_TELEGRAM_API_HASH` 读取参数，或者从文件 `~/.config/panim/telegram.env` 读取。
- 没有参数时，构建仍然成功。这时应用会在设置中要求用户填写自己的参数。
- 要在没有参数的情况下构建，把 `PANIM_TELEGRAM_ENV` 设为空文本。
- `docker/check.sh --values FILE` 用 FILE 中的参数制作发布的安装包（见“制作发布版本”一节）。构建容器只以只读方式读取 FILE。脚本永远不会打印这些参数。
- 工作流 `macos-release.yml` 用开发仓库的两个 secret 中的参数制作发布的 `.dmg`。它永远不会打印这些参数。
- 测试不需要这些参数。

永远不要把 Telegram 参数放进仓库、issue 或日志中。永远不要打印它们。如果你用自己的参数做测试，把它们放在 `~/.config/panim/telegram.env` 或环境变量中。

## 规则

每次修改都要遵守这些规则。

### 写作规则

所有文档都用 ASD-STE100 简化技术英语（STE）书写。这条规则适用于 `docs/` 和 `base/` 中的文件、`README.md`、`CONTRIBUTING.md`、`SECURITY.md`、`CHANGELOG.md`、文档注释（`///` 和 `//!`）以及代码中的注释。

1. 只使用 STE 词典中批准的词，并使用批准的词义。可以使用技术名称（例如 `TDLib`、`IMAP`、"proxy"）和技术动词（例如 "compile"）。
2. 一个词只表示一个意思。同一个事物每次都用同一个词。
3. 步骤中的句子不超过 20 个词，描述中的句子不超过 25 个词。
4. 每个句子只写一条指令。指令使用祈使句（例如 "Run the test"）。
5. 使用主动语态。只使用一般现在时、一般过去时和一般将来时。
6. 不要使用动词的 "-ing" 形式，技术名称除外。
7. 名词串不超过三个词。
8. 不要为了缩短句子而省略冠词（"the"、"a"）。
9. 每个段落只写一个主题。每个段落不超过六个句子。
10. 一系列步骤或一组条目使用竖排列表。

### 中文文档

`README.md`、`CONTRIBUTING.md` 和 `SECURITY.md` 各有一个中文版：`README.zh-CN.md`、`CONTRIBUTING.zh-CN.md` 和 `SECURITY.zh-CN.md`。

- 修改这三个英文文件中的任何一个时，必须在同一个提交中同时修改对应的中文文件。
- 英文文件是参考版本。如果两个文件不一致，以英文文件为准。
- STE 不适用于中文文本。用简短、清楚的中文书写，章节、命令、路径和链接与英文文件相同。
- 使用中文界面中的用词（`crates/app/src/lib/i18n.svelte.ts`），例如 身份（persona）、口令（passcode）和 找回词（recovery words）。

### 用户界面

- 用户界面的每段文本都有英文和中文，位于 `crates/app/src/lib/i18n.svelte.ts`。每段新文本都要用两种语言添加。
- 如果某种语言缺少文本，`pnpm check` 会失败。

### 平台

- 遵守每个平台的规则。
- 不要抓取网页。
- 不要通过机器人专用的 API 控制用户账户。
- 如果 API 不提供访问，PANIM 在浏览器视图中显示该账户。

### 规范

`base/` 包含规范，例如 `base/panim-proof-v1.md`。其他工具使用它们，所以不要修改已发布的版本。请制作新版本。

### 第 1 步

PANIM 处于第 1 步（`docs/DESIGN.md` §3）。第 1 步中每个人只有一个身份，用户界面不显示它。把身份作为账户、浏览器配置和网络路由的键。第 1 步不使用 ZIKARON。

## 目录结构

- `crates/` 包含 Rust 工作区：
  - `panim-core` 包含数据模型、身份密钥和证明。它没有用户界面、网络代码和磁盘代码。
  - `panim-vault` 包含加密的本地数据：保险库、加密存储、加密的机密、加密的浏览器配置、密钥文件和备份。
  - `panim-github` 包含 GitHub 连接器的网络代码。
  - `panim-telegram` 包含 Telegram 连接器的代码：`tdjson` 的薄封装层、登录、聊天和聊天面板。
  - `app` 是桌面应用。`app/src` 是 Svelte 用户界面。`app/src-tauri` 是 Rust 部分（crate `panim`）。
- `base/` 包含规范。
- `docs/` 包含设计、架构和路线图。
- `nix/` 包含软件包、补丁和发布脚本：
  - `package.nix` 是 Nix 软件包。
  - `tdlib-no-direct.patch` 是 PANIM 版 TDLib 的补丁。文件中说明了每处修改。
  - `release.sh` 写入并签名发布版本的校验和。`publish.sh` 制作发布版本的公开提交。
- `scripts/` 包含不用 Nix 构建的脚本：
  - `build-tdlib.sh` 用 `nix/` 中的补丁构建 PANIM 版 TDLib。
  - `build-packages.sh` 用 Tauri 的打包工具构建软件包。`crates/app/src-tauri/bundle/` 包含 Linux 和 macOS 的打包设置。
- `docker/` 包含在容器中检查不用 Nix 的构建的脚本（`check.sh`）。
- `.github/ISSUE_TEMPLATE/` 包含公开仓库的 issue 表单。
- `README.zh-CN.md`、`CONTRIBUTING.zh-CN.md` 和 `SECURITY.zh-CN.md` 是三个公开文档的中文版。
- `flake.nix` 是 Nix flake。`rust-toolchain.toml` 设置 Rust 版本。开发 shell 和 Nix 软件包都使用这个版本。

## 报告问题

- 在 https://github.com/SakuraRainn/panim/issues 开一个 issue，选择问题报告表单或提问表单。
- 写明 PANIM 的版本、你的系统、安装方式（`.deb`、`.rpm`、AppImage 或 Nix）、操作步骤和结果。
- 不要在 issue 中写口令、找回词、密钥文件、令牌或 Telegram 参数。
- 安全漏洞不要开公开的 issue。请阅读 `SECURITY.zh-CN.md`。

## 修改如何进入 PANIM

公开仓库只包含发布版本，所以它不能合并 pull request。修改按以下顺序进入 PANIM：

1. 先开一个 issue。说明问题和你想做的修改。这样维护者可以告诉你这个修改是否符合计划。
2. 在最新的发布版本上做修改，并运行测试（见“构建和测试”一节）。
3. 发送修改。使用以下方法之一：
   - 在公开仓库中开一个 pull request。
   - 把 `git format-patch` 生成的补丁附在 issue 中。
4. 维护者审查修改，并在开发仓库中应用它。该提交会在 `Co-authored-by` 行中包含你的名字和邮箱。
5. 修改随下一个发布版本发布。然后维护者关闭你的 pull request 或 issue。

一个发布版本的公开提交只有一个提交，所以它不显示开发仓库中的提交。`CHANGELOG.md` 中该版本的条目会列出本版本的贡献者。

发送修改即表示你同意 PANIM 按照 PANIM 的 MIT 许可证（`LICENSE`）使用它。

## 制作发布版本

以下是维护者的步骤。在开发仓库中进行。`nix` 命令在 `nix develop` 的 shell 中运行。

在第一个包含 macOS 的发布版本之前，做一次以下设置：为开发仓库添加工作流 `macos-release.yml` 需要的两个 secret：

```
gh secret set PANIM_TELEGRAM_API_ID
gh secret set PANIM_TELEGRAM_API_HASH
```

每个命令都会询问参数，并从标准输入读取它。不要在命令行中给出参数（例如用 `--body`），因为 shell 历史会保存它。

1. 修改所有 `Cargo.toml` 文件、`crates/app/package.json`、`crates/app/src-tauri/tauri.conf.json` 和 `Cargo.lock` 中的版本。在 `CHANGELOG.md` 中写入该版本的条目。
2. 确认所有测试都通过。提交修改，并创建标签 `v<version>`。
3. 在一个干净的工作区中检出标签 `v<version>`。删除文件夹 `target/container` 和 `target/macos`，这样里面不会有旧版本的安装包。
4. 构建带 PANIM 的 Telegram 参数的安装包。这一步需要 Docker 或 rootless Podman：

   ```
   docker/check.sh all --values ~/.config/panim/telegram.env
   ```

   - 第一次运行大约需要 30 分钟。
   - 如果工作区有修改，或者 `HEAD` 没有与 `tauri.conf.json` 版本一致的标签 `v<version>`，脚本会拒绝启动。
   - 构建容器只以只读方式读取参数文件。镜像、报告和日志中都没有这些参数。
   - 构建、参数检查和安装检查都必须通过。安装包在 `target/container/packages` 中。
   - 这时参数会进入每个安装包的二进制文件中。
5. 在 GitHub 的 macOS 运行器上构建 macOS 的 `.dmg`。macOS 的一分钟大约相当于 Linux 的 10 分钟，所以只运行一次。推送标签，并把它的提交推送到分支 `macos-release`：

   ```
   git push --atomic origin v<version> '+v<version>^{commit}:refs/heads/macos-release'
   gh run list --workflow macos-release.yml --limit 1
   gh run watch <run>
   gh run download <run> -n macos-release-dmg -D target/macos
   sha256sum target/macos/PANIM_<version>_aarch64.dmg
   ```

   - `^{commit}` 是必需的，因为 GitHub 不接受附注标签（annotated tag）作为分支的值。
   - 如果提交没有与 `tauri.conf.json` 版本一致的标签 `v<version>` 或 `v<version>-rc.<n>`，工作流会拒绝它。
   - 一次运行大约需要 30 分钟。分支上的第一次运行还要构建 TDLib，大约多需要 20 分钟。
   - 运行中的所有检查都必须通过。把 SHA-256 与运行摘要页面中的 `SHA256` 行比较。
6. 用私钥写入并签名校验和。私钥保存在仓库之外：

   ```
   nix run .#release -- --version <version> --ref v<version> --key ~/.minisign/panim.key \
     --out /path/to/release \
     --artifact target/container/packages/PANIM_<version>_amd64.deb \
     --artifact target/container/packages/PANIM-<version>-1.x86_64.rpm \
     --artifact target/container/packages/PANIM_<version>_amd64.AppImage \
     --artifact target/macos/PANIM_<version>_aarch64.dmg \
     --pubkey ~/.minisign/panim.pub
   ```

7. 把公开仓库克隆到一个新文件夹：`git clone https://github.com/SakuraRainn/panim /path/to/panim-public`。
8. 把 PANIM 的 Telegram 参数放入环境变量，不要放进仓库。这样下一步的脚本可以确认 `api_hash` 不在提交中：

   ```
   set -a; . ~/.config/panim/telegram.env; set +a
   ```

9. 用签名的源代码归档制作公开提交和标签：

   ```
   nix run .#publish -- --from /path/to/release --clone /path/to/panim-public --version <version> \
     --name <name> --email <id>+<name>@users.noreply.github.com --pubkey ~/.minisign/panim.pub
   ```

   - 作者和提交者使用 GitHub 的私密邮箱。脚本拒绝其他邮箱。
   - 脚本用归档中的文件替换克隆中的所有文件，并删除新版本中没有的文件。
   - 它确认文件树与归档完全相同、`export-ignore` 排除的文件不存在、`api_hash` 不存在。如果某项检查失败，它会再次删除提交和标签。
   - 它不推送。
10. 检查提交。然后运行脚本显示的推送命令。
11. 在公开仓库中，在标签 `v<version>` 上创建发布。发布输出文件夹中的所有文件：`.deb`、`.rpm`、AppImage、`.dmg`、`panim-<version>-source.tar.gz`、`SHA256SUMS` 和 `SHA256SUMS.minisig`。
12. 推送开发仓库的分支。第 5 步已经推送了标签。

候选版本（release candidate）用于在 Mac 上的手工检查（`docs/ROADMAP.md` §10）。在提交上创建标签 `v<version>-rc.<n>`。只进行第 5 步，并在 `git push` 命令中用 `v<version>-rc.<n>` 代替 `v<version>`。这样 `.dmg` 就包含 PANIM 的 Telegram 应用参数。不要发布候选版本。

`.gitattributes` 标记只用于开发的文件：`git archive` 会排除它们。因此，源代码归档和公开仓库中没有这些文件。不要这样标记 `README.md`、`CONTRIBUTING.md`、`SECURITY.md`、它们的中文文件或 `.github/`。
