# Moss Agent

Coming soon! This repository will contain the code and documentation for the Moss Agent project.

## Moss 执行器 vs ClawHub 需求覆盖率（Awesome 列表，2,868 个精选，2026-02-07）

### Moss 当前执行器能力

| 执行器 | 支持的扩展名 | 隔离级别 | 依赖 |
|--------|------------|---------|------|
| Native | 无扩展（二进制） | 无 | 无 |
| Deno | `.js` `.ts` `.jsx` `.tsx` `.mjs` `.mts` | Runtime | Deno 二进制 |
| Docker | `.py` `.sh` `.bash` `.rb` | Container | Docker 守护进程 |
| Wasmtime | `.wasm` `.wat` | Runtime | 内置 |

### 覆盖率评估

| ClawHub skill 类型 | 占比估算 | Moss 兼容性 | 说明 |
|--------------------|---------|------------|------|
| Prompt-only（纯指令） | 60-75% | ✅ 直接兼容 | 只需正确注入 SKILL.md |
| Shell/Bash 脚本 | 10-15% | ✅ Docker 执行器 | 需要 Docker 运行 |
| Python 脚本 | 5-10% | ✅ Docker 执行器 | 需要 Docker 运行 |
| Node.js 脚本 | 5-10% | ⚠️ 部分支持 | Docker 可执行单文件，但 npm 依赖不支持 |
| 原生二进制（brew/go） | <5% | ⚠️ 需用户手动安装 | NativeExecutor 可执行，但依赖需预装 |

**综合覆盖率：75-90% 的 ClawHub skills 可在 Moss 上运行。**
