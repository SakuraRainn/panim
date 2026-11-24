# PANIM

[English](README.md) | 简体中文

本文件是 `README.md` 的中文版。如果两个文件的内容不一致，以英文版 `README.md` 为准。

PANIM（希伯来语 פנים，意思是“面孔”，这个词在希伯来语中总是复数）是一个 Linux 和 macOS 桌面身份管理器。你把自己的账户连接到 PANIM，然后在同一个地方、在你自己的电脑上阅读和回复它们。PANIM 没有服务器。

以后，PANIM 可以让一个人分成多个身份。每个身份都有自己的账户、浏览器配置和网络路由，彼此分开。目前 PANIM 只有一个身份，应用界面不显示它。

`CHANGELOG.md` 说明每个版本的内容。`docs/DESIGN.md` 说明完整的计划。

## 账户

| 账户 | PANIM 如何显示 |
|---|---|
| GitHub | 在时间线中：通知，以及你关注的仓库的事件。你可以回复公开仓库中的 issue 或 pull request。 |
| Telegram | 在时间线中：主聊天列表中的每个聊天是一项，显示最后一条消息和未读数。聊天面板显示文字消息，并可以发送文字。 |
| X | 在浏览器视图中 |
| Discord | 在浏览器视图中 |
| Proton Mail | 在浏览器视图中 |

- **时间线**把 GitHub 和 Telegram 的更新放在一起显示。PANIM 把它们保存在你的电脑上。
- **浏览器视图**在 PANIM 的私密浏览器配置中打开该服务的网页。浏览器配置会保留你的登录。PANIM 只从页面读取未读数，不读取你的消息。

PANIM 遵守每个平台的规则。它不抓取网页，也不通过机器人专用的 API 控制你的账户。PANIM 不支持 Facebook、Instagram、Threads 和 WhatsApp。

## 只有官方发布版本才是 PANIM

其他应用可以使用 PANIM 的名字和外观。只有官方仓库发布的版本才是 PANIM。官方仓库是 https://github.com/SakuraRainn/panim。每个发布版本都有一个 `SHA256SUMS` 文件及其 minisign 签名 `SHA256SUMS.minisig`。启动下载的文件之前，请先检查它（见下一节）。

PANIM 的 minisign 公钥（密钥 ID `7B5C5C4A551F160B`）：

```
RWQLFh9VSlxce6HvAgce8fOpN5K41RRR5zeh8KsFVMPIDtdmRGD1MkXa
```

不要信任其他来源的公钥。消息、issue 或其他网站中的公钥都不是 PANIM 的公钥。

## 检查下载的文件

1. 从发布页面下载你需要的文件，以及 `SHA256SUMS` 和 `SHA256SUMS.minisig`，放在同一个文件夹中。
2. 安装 minisign，例如 `sudo apt install minisign`、`nix shell nixpkgs#minisign`，或者在 macOS 上运行 `brew install minisign`。
3. 检查签名：`minisign -Vm SHA256SUMS -P RWQLFh9VSlxce6HvAgce8fOpN5K41RRR5zeh8KsFVMPIDtdmRGD1MkXa`。结果必须是 "Signature and comment signature verified"。可信注释（trusted comment）给出版本和提交。
4. 检查文件：`sha256sum -c SHA256SUMS --ignore-missing`。在 macOS 上使用 `shasum -a 256 -c SHA256SUMS --ignore-missing`。你下载的每个文件都必须显示 "OK"。
5. 如果某一步失败，不要启动该文件。从官方仓库重新下载。

## 安装

PANIM 运行在 Linux x86_64 上，以及 Apple 芯片（Apple silicon）的 macOS 14 或更高版本上。

- **Linux。** 安装包需要 glibc 2.39 或更高版本，例如 Ubuntu 24.04、Debian 13 或 Fedora 40 及更高版本。从下面的方法中选择一种：`.deb`、`.rpm`、AppImage 或 Nix。
- **macOS。** 使用 `.dmg`。使用 Intel 处理器的 Mac 无法启动它。

`.deb`、`.rpm`、AppImage 和 `.dmg` 都包含 PANIM 的 Telegram 应用参数（见“Telegram”一节）。

### .deb（Ubuntu 和 Debian）

1. 下载 `PANIM_<version>_amd64.deb` 并检查它（见“检查下载的文件”一节）。
2. 运行 `sudo apt install ./PANIM_<version>_amd64.deb`。Apt 会同时安装 PANIM 需要的库。
3. 从应用程序菜单启动 PANIM，或者运行 `panim`。

### .rpm（Fedora）

1. 下载 `PANIM-<version>-1.x86_64.rpm` 并检查它（见“检查下载的文件”一节）。
2. 运行 `sudo dnf install ./PANIM-<version>-1.x86_64.rpm`。Dnf 会同时安装 PANIM 需要的库。
3. 从应用程序菜单启动 PANIM，或者运行 `panim`。

### AppImage

AppImage 是一个文件。它包含 WebKitGTK、GTK 和 PANIM 的其他库。它使用你系统中的图形库和字体。

1. 下载 `PANIM_<version>_amd64.AppImage` 并检查它（见“检查下载的文件”一节）。
2. 把它放在只有你能写入的文件夹中，例如 `~/Applications`。
3. 运行 `chmod +x PANIM_<version>_amd64.AppImage`。
4. 启动该文件。

如果 AppImage 不能正常启动：

- **"No suitable fusermount binary found"**：安装 FUSE 3（在 Ubuntu、Debian 和 Fedora 上是软件包 `fuse3`）。没有 FUSE 3 时，AppImage 每次都会把自己解压到一个临时文件夹中，启动会更慢。
- **窗口一直是空白的**：用 `WEBKIT_DISABLE_DMABUF_RENDERER=1` 启动 AppImage。如果仍然不行，再设置 `LIBGL_ALWAYS_SOFTWARE=1`。

0.1.x 版本的 AppImage 需要用户命名空间，在 Ubuntu 24.04 上还需要一个 AppArmor 配置文件。0.2.0 版本的 AppImage 不再需要它们。要删除旧的配置文件，运行 `sudo apparmor_parser -R /etc/apparmor.d/panim`，然后运行 `sudo rm /etc/apparmor.d/panim`。

### .dmg（macOS）

1. 下载 `PANIM_<version>_aarch64.dmg` 并检查它（见“检查下载的文件”一节）。
2. 打开 `.dmg`，把 PANIM 移到“应用程序”文件夹。
3. 启动 PANIM。第一次启动时，macOS 会阻止它，因为 PANIM 没有 Apple 的签名。关闭提示。不要把 PANIM 移到废纸篓。
4. 打开“系统设置 > 隐私与安全性”。在“安全性”下面，点击 PANIM 的“仍要打开”，然后输入你的密码。

macOS 只询问一次。在 macOS 15 及更高版本上，按住 Control 键点击应用不能再用这种方式打开它。请使用“系统设置”。

PANIM 把数据保存在 `~/Library/Application Support/panim` 中。PANIM 运行时，把打开的浏览器配置放在一个 RAM 磁盘上。访达（Finder）不显示这个磁盘。用 Cmd+Q 退出 PANIM：PANIM 会加密保存数据，并移除 RAM 磁盘。

### 使用 Nix

如果你有启用了 flakes 的 Nix：

1. 运行 `nix run github:SakuraRainn/panim` 启动 PANIM。
2. 或者运行 `nix build github:SakuraRainn/panim`，得到 `result/bin/panim`。

从源代码构建的版本没有 PANIM 的 Telegram 应用参数。要添加 Telegram 账户，请填写你自己的参数（见“Telegram”一节）。

## 首次启动

1. 选择语言：英文或中文。
2. 设置一个至少 8 个字符的口令。每天用口令打开 PANIM。
3. PANIM 显示你的 12 个找回词。按顺序把它们抄在纸上。把纸妥善保管在电脑之外。任何拿到这些词的人都可以冒充你。
4. PANIM 会要求你输入其中 4 个词。核对通过后，首次启动才完成。
5. 如果你想要身份密钥的另一份副本，请保存一个密钥文件。它有单独的密码。把它保存在 PANIM 之外。

口令、12 个找回词和密钥文件都可以打开 PANIM：

- 如果忘记口令，点击“忘记口令？”。使用 12 个找回词或密钥文件，然后设置新口令。
- 连续输错 5 次口令后，只能用 12 个找回词或密钥文件打开 PANIM。
- 15 分钟没有操作后，PANIM 会锁定。你可以在设置中更改这个时间。

PANIM 把所有数据加密保存在 `~/.local/share/panim` 中（在 macOS 上是 `~/Library/Application Support/panim`）。没有口令、12 个找回词或密钥文件，任何人都无法打开它。PANIM 的维护者也无法打开。

## Telegram

Telegram 通过应用参数识别每个应用：一个 `api_id` 和一个 `api_hash`。

- **PANIM 的应用参数。** `.deb`、`.rpm` 和 AppImage 都包含这组参数。所有使用 PANIM 的人共用这组参数，因此它不会关联你的各个身份。这是默认设置。
- **你自己的应用参数。** 在 my.telegram.org → API development tools 获取。在 PANIM 中打开“设置 → Telegram 应用参数”，选择“我自己的应用参数”并填写。PANIM 把它们加密保存，并且不再显示 `api_hash`。
- **警告：** 一个 `api_id` 属于一个 Telegram 账户。如果多个身份的登录使用你自己的 `api_id`，Telegram 就能看出它们属于同一个人。
- 更改这项设置只影响新的登录。每个账户保留它登录时使用的参数。

## 网络路由

在“设置 → 网络路由”中选择 PANIM 的连接方式：直连、HTTP 代理或 SOCKS5 代理。GitHub、Telegram 和浏览器视图都使用这条路由。使用 Tor 时，选择 SOCKS5，地址 `127.0.0.1`，端口 `9050`。

PANIM 使用的 Telegram 库经过修改，因此 Telegram 不会绕过代理建立连接。

## 备份

1. 在 PANIM 中打开“设置 → 备份”，点击“备份…”。选择文件的保存位置。
2. 妥善保管这个文件。它保持加密。只有你的口令、12 个找回词或密钥文件能打开它。
3. 要恢复备份，在没有 PANIM 数据的电脑上启动 PANIM。在第一个界面点击“从备份恢复”，然后选择该文件。
4. 先关闭旧电脑上的 PANIM，或者在旧电脑上移除 Telegram 账户。否则，同样的会话会同时存在于两个地方。

## 已知限制

- `.dmg` 只适用于 Apple 芯片，并且没有 Apple 的签名（见“.dmg（macOS）”一节）。
- 所有浏览器视图都具有和你的电脑相同的指纹。PANIM 的浏览器配置和路由只能阻止简单的身份关联。
- 如果你的电脑有交换空间（swap），系统可能把打开的浏览器配置写入交换空间。请使用加密的交换空间，或者不使用交换空间。
- 浏览器视图的未读数只在其窗口打开时存在。
- Telegram 聊天面板只显示文字。在屏蔽 Telegram 地址的网络中，请使用代理。
- 安装包使用你系统中的图形驱动。还没有人用 NVIDIA 专有驱动测试过 PANIM。

`CHANGELOG.md` 列出每个版本的全部限制。

## 帮助

- **问题或疑问：** 在 https://github.com/SakuraRainn/panim/issues 开一个 issue。选择问题报告表单或提问表单。
- **安全漏洞：** 不要开公开的 issue。请阅读 `SECURITY.zh-CN.md`。
- **修改 PANIM：** 请阅读 `CONTRIBUTING.zh-CN.md`。它说明如何构建 PANIM、规则，以及修改如何进入 PANIM。

## 许可证

PANIM 使用 MIT 许可证。见 `LICENSE`。
