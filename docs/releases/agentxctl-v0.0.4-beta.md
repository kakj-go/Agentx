# Agentx v0.0.4-beta

本版本同步发布 Windows/Linux x64 的 agentxctl 与全部 11 个 Linux AMD64 集群镜像。`agentxctl 0.0.4-beta` 已嵌入对应的 Helm Chart、Schema 和 `kakj/agentx-*:v0.0.4-beta` 镜像配置；下载校验后执行 `install` 即可部署，无需源码仓库或额外安装脚本。

## 本版本内容

- Workflow 画布插件支持动态 UI、权威动态输入输出契约与调用隔离。
- 修复插件进程回收、Worker 并发、文件流读写、Trace 降级和详情缓存问题。
- 插件 SDK API 与 Runner RPC 升级到 2，每次调用使用独立进程和文件目录。
- README 提供新版 ctl 直接下载、SHA-256 校验和安装命令。
- 发布前验证 CLI、Chart、README 与镜像版本一致，并检查全部公开镜像；资产完整上传到草稿后才公开 Release。

## 安装和升级

仍需预先安装 Helm 3、kubectl，准备具有默认 StorageClass 的 Kubernetes 集群，并独立安装可被 Runtime 访问的 OpenSandbox。

Windows 下载 `agentxctl-windows-x86_64.exe`，Linux 下载 `agentxctl-linux-x86_64`，并校验对应 `.sha256`。Linux 首次执行前运行 `chmod +x agentxctl-linux-x86_64`。

- 新部署：运行新版 ctl 的 `install`。
- 已有部署：评估版本差异后运行新版 ctl 的 `upgrade`。自定义 `--values` 需同步更新镜像标签。
- 验证：运行 `status --output json` 和 `doctor`。

当前仍为开发阶段 Beta，不保证旧数据和协议兼容。已有自定义插件需使用 SDK API 2 重新构建后导入。OpenSandbox 不由本 Release 安装或升级。

本 Release 附带二进制、归档、SHA-256 文件和 `release-images.json`，后者记录本次验证的 11 个公开镜像及其 linux/amd64 manifest 摘要。
