# PANIM

[English](README.md) | 简体中文

本文件是 `README.md` 的中文版。如果两个文件的内容不一致，以英文版 `README.md` 为准。

PANIM（希伯来语 פנים，意思是“面孔”，这个词在希伯来语中总是复数）是一个 Linux 桌面身份管理器。你把自己的账户连接到 PANIM，然后在同一个地方、在你自己的电脑上阅读和回复它们。PANIM 没有服务器。

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
2. 安装 minisign，例如 `sudo apt install minisign` 或 `nix shell nixpkgs#minisign`。
3. 检查签名：`minisign -Vm SHA256SUMS -P RWQLFh9VSlxce6HvAgce8fOpN5K41RRR5zeh8KsFVMPIDtdmRGD1MkXa`。结果必须是 "Signature and comment signature verified"。可信注释（trusted comment）给出版本和提交。
4. 检查文件：`sha256sum -c SHA256SUMS --ignore-missing`。你下载的每个文件都必须显示 "OK"。
5. 如果某一步失败，不要启动该文件。从官方仓库重新下载。

## 安装

PANIM 运行在 Linux x86_64 上。从下面两种方法中选择一种：AppImage 或 Nix。

### AppImage

AppImage 是一个文件。它包含 PANIM 需要的所有库，以及 PANIM 的 Telegram 应用参数（见“Telegram”一节）。

1. 下载 `PANIM-<version>-x86_64.AppImage` 并检查它（见“检查下载的文件”一节）。
2. 把它放在只有你能写入的文件夹中，例如 `~/Applications`。
3. 运行 `chmod +x PANIM-<version>-x86_64.AppImage`。
4. 启动该文件。

如果 AppImage 无法启动：

- **"cannot unshare"**：你的系统阻止了非特权用户命名空间。见下一节。
- **"Cannot mount AppImage"**：安装 FUSE 3（在 Ubuntu、Debian 和 Fedora 上是软件包 `fuse3`）。或者用 `--appimage-extract-and-run` 选项启动 AppImage。
- **窗口一直是空白的**：用 `WEBKIT_DISABLE_DMABUF_RENDERER=1` 启动 AppImage。如果仍然不行，再设置 `LIBGL_ALWAYS_SOFTWARE=1`。

### Ubuntu 24.04 及更高版本

AppImage 必须创建一个用户命名空间。Ubuntu 24.04 及更高版本用 AppArmor 阻止非特权用户命名空间（`kernel.apparmor_restrict_unprivileged_userns = 1`）。这时 AppImage 会停止并显示 "cannot unshare: Operation not permitted"，同时用它自己的路径显示下面的步骤。

安全的解决方法是添加一个 AppArmor 配置文件，只允许 PANIM 的 AppImage 创建用户命名空间。Ubuntu 对需要用户命名空间的应用也使用这种方法。你电脑上的其他程序仍然受到保护。

1. 找到 AppImage 的完整路径，例如 `/home/you/Applications/PANIM-0.1.1-x86_64.AppImage`。
2. 写入配置文件。把你的路径放在引号之间：

   ```
   sudo tee /etc/apparmor.d/panim <<'EOF'
   abi <abi/4.0>,
   include <tunables/global>
   profile panim "/home/you/Applications/PANIM-0.1.1-x86_64.AppImage" flags=(unconfined) {
     userns,
     include if exists <local/panim>
   }
   EOF
   ```

3. 加载配置文件：`sudo apparmor_parser -r /etc/apparmor.d/panim`。
4. 再次启动 AppImage。

请注意：

- 配置文件只对一个路径有效。如果你移动或重命名 AppImage，请修改配置文件中的路径，然后再做一次第 3 步。
- 为了以后的版本，你可以在路径中写模式 `PANIM-*-x86_64.AppImage`。只在只有你能写入的文件夹中使用模式。
- 配置文件不会以其他方式限制 PANIM。它只允许 PANIM 创建用户命名空间。
- 要删除配置文件，运行 `sudo apparmor_parser -R /etc/apparmor.d/panim`，然后运行 `sudo rm /etc/apparmor.d/panim`。
- `--appimage-extract-and-run` 选项没有帮助。它运行同样的第一步，这一步同样需要用户命名空间。
- 不要为所有程序关闭这项保护（`sysctl kernel.apparmor_restrict_unprivileged_userns=0`）。配置文件就足够了。
- 你也可以用 Nix 安装 PANIM（见下一节）。Nix 软件包不需要用户命名空间。

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

PANIM 把所有数据加密保存在 `~/.local/share/panim` 中。没有口令、12 个找回词或密钥文件，任何人都无法打开它。PANIM 的维护者也无法打开。

## Telegram

Telegram 通过应用参数识别每个应用：一个 `api_id` 和一个 `api_hash`。

- **PANIM 的应用参数。** AppImage 包含这组参数。所有使用 PANIM 的人共用这组参数，因此它不会关联你的各个身份。这是默认设置。
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

- PANIM 只能在 Linux 上运行。macOS 版本会在以后的版本中提供。
- 所有浏览器视图都具有和你的电脑相同的指纹。PANIM 的浏览器配置和路由只能阻止简单的身份关联。
- 自动锁定只统计主窗口中的操作。
- 如果你的电脑有交换空间（swap），系统可能把打开的浏览器配置写入交换空间。请使用加密的交换空间，或者不使用交换空间。
- 浏览器视图的未读数只在其窗口打开时存在。
- Telegram 聊天面板只显示文字。在屏蔽 Telegram 地址的网络中，请使用代理。
- AppImage 包含 Mesa 图形驱动。还没有人用 NVIDIA 专有驱动测试过它。

`CHANGELOG.md` 列出每个版本的全部限制。

## 帮助

- **问题或疑问：** 在 https://github.com/SakuraRainn/panim/issues 开一个 issue。选择问题报告表单或提问表单。
- **安全漏洞：** 不要开公开的 issue。请阅读 `SECURITY.zh-CN.md`。
- **修改 PANIM：** 请阅读 `CONTRIBUTING.zh-CN.md`。它说明如何构建 PANIM、规则，以及修改如何进入 PANIM。

## 许可证

PANIM 使用 MIT 许可证。见 `LICENSE`。
