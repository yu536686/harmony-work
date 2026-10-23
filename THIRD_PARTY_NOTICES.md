# 第三方组件说明

根目录的 [MIT 许可证](LICENSE) 适用于 Harmony Work 项目的原创代码与文档，不取代第三方组件自身的许可。分发第三方源码或二进制时，应保留其原始版权、许可证和要求附带的声明。

## 已有声明与主要组件

- **Codex / Codex SDK**：由 OpenAI 提供，采用 Apache-2.0。项目通过 Codex app-server、Codex exec 和 `@openai/codex-sdk` 集成；应保留实际分发版本中的许可证与 NOTICE（如有）。上游：https://github.com/openai/codex
- **FlaUI-MCP**：现有 MIT 声明见 [installer/licenses/FlaUI-MCP.LICENSE](installer/licenses/FlaUI-MCP.LICENSE)。
- **Node.js**：现有运行时声明见 [src-tauri/licenses/node-v24.18.0-LICENSE](src-tauri/licenses/node-v24.18.0-LICENSE)，其中包含 Node.js 及其所含第三方组件的声明。
- **Office Artifact Runtime**：docx、PptxGenJS、ExcelJS、JSZip 的现有声明见 [THIRD_PARTY_NOTICES.txt](src-tauri/resources/artifact-runtime/THIRD_PARTY_NOTICES.txt)。

## 发布与打包

以上为现有声明入口和主要组件摘要，不是所有直接、间接依赖的完整许可清单。npm 与 Cargo 依赖以锁文件及相应版本附带的许可为准。安装器还可能分发 Python、Git、WebView2、WeCom CLI 及自动化组件；这些组件须按各自条款分发，不能将其统一声明为 MIT。

发布者应按实际打包内容携带相应的第三方许可证与声明。源码仓库的许可证更新不会自动更新或重新许可已经发布的安装包。

Harmony Work 的名称、Logo、商标以及外部在线服务的访问权，不因项目代码许可而获得额外授权。
