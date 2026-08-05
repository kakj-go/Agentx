# MinIO 与外部 S3

`bundled-minio` 使用 MinIO StatefulSet、`data-minio-0` PVC 和 Bucket 初始化 Job。根凭据复用通用 `AGENTX_S3_ACCESS_KEY/SECRET_KEY` 键。

`external-s3` 不创建 Bucket，支持 Session Token、Path/Virtual-host Style、HTTP 开发端点、HTTPS 和私有 CA Bundle。生产 Profile 禁止 `allowHttp=true`。Doctor 向 Profile 指定 Bucket 写入临时对象并立即删除；失败时不会继续 Migration 或核心服务部署。

基础设施的安装、升级和卸载统一使用 `deploy.ps1 -Target infrastructure`。脚本只管理 bundled MinIO；外部 Bucket、对象和凭据不会被升级或卸载命令修改。
