# PRX-Memory 自我进化记忆系统：从工程治理到可验证演进的本地优先 MCP 实践

## 摘要

本文提出并论证 `prx-memory` 的核心命题：长期记忆系统不仅要“能存能检索”，还必须具备“可治理、可验证、可演进”的工程属性。我们基于本地优先（local-first）和 MCP 协议，将记忆系统拆分为治理层、检索层、演进层三个协同子系统，并给出一个可执行的最小自我演进框架（MSES, Minimal Self-Evolving System）。该框架通过 train-holdout 双集合接受准则、成本与风险惩罚、硬约束门控，实现对策略变体的可验证选择，避免“只在训练集上变好”的伪进化。本文同时提供关键工程故事、代码落地映射、可复现实验命令与上线可行性分析。

关键词：MCP、长期记忆、治理、可解释演进、holdout、local-first、Rust

## 1. 问题定义

当前多数 Agent memory 方案在实践中会退化为三类问题：

1. 日志化污染：记忆库被原始对话和临时噪声占据，检索精度下降。
2. 抽象失衡：过度“原则化”导致 decision 类膨胀，无法稳定指导执行。
3. 演进失真：策略更新没有通用化校验，出现训练集收益与线上退化并存。

`prx-memory` 的目标不是“再造一个向量库”，而是构建一个带治理能力和演进能力的记忆执行系统，使其在 Codex、Claude Code、OpenClaw、OpenPRX 等客户端间具备统一行为。

## 2. 研究问题与可证伪假设

### 2.1 研究问题

1. 本地优先 MCP 记忆系统能否在多客户端环境中维持稳定契约？
2. 治理规则能否从“文档建议”转化为“工具级强约束”？
3. 自我进化是否能在工程系统中形成可验证闭环，而非概念口号？

### 2.2 假设

- H1：若 memory 写入受治理规则和比例约束，检索污染可被长期抑制。
- H2：若策略接受采用 train + holdout 双集合改进准则，则可显著降低伪进化风险。
- H3：若演进输出与记忆维护流程联动（去重、压缩、回归验证），系统可长期保持可用。

## 3. 重点故事：从“重复踩坑”到“可复用知识”

### 3.1 背景

在插件化开发场景中，常出现“代码已改，行为未变”的假象。根因通常是运行时缓存、加载路径、配置重载次序等隐性状态。该类问题具有三个特征：

1. 复发频率高。
2. 排查成本高。
3. 容易误导为“逻辑错误”。

### 3.2 传统处理方式的失败

传统做法是每次重试，偶尔记录到个人笔记。结果是：

1. 无结构，难检索。
2. 无治理，容易重复写入同义条目。
3. 无验证，无法确认知识是否真正可召回。

### 3.3 `prx-memory` 的处理方式

同类问题在 `prx-memory` 中被强制映射为可复用条目：

1. 技术层（fact）：`Pitfall / Cause / Fix / Prevention`。
2. 原则层（decision）：在具备泛化意义时才生成。
3. 写入后校验：关键路径会触发 recall 验证与去重。
4. 周期维护：每 100 次写入自动触发去重和比例再平衡。

这使“经验”从一次性文本，转化为可被未来命中的工程资产。

## 4. 系统模型

## 4.1 三层结构

1. 治理层（Governance）
- 负责格式、标签、类别、比例、去重、写后验证等约束。

2. 检索层（Retrieval）
- 负责 lexical + vector 融合召回、可选远端 rerank、ACL scope 过滤。

3. 演进层（Evolution）
- 负责候选策略评估、接受决策、holdout 一致性检查。

## 4.2 理论锚点：从本质到演进

本文显式采用三重理论锚点：

1. 第一性原理层（本质与约束）
- 先定义不可违反的底层约束与可测目标。
- 在本系统中，对应 \(G\)、\(\Pi\) 与带约束评估的 \(\mathcal{J}\)。

2. 达芬奇式结构迁移层（结构化重组）
- 通过跨模块重组与流程重排生成候选结构。
- 在本系统中，对应基于 \(\mathcal{M}\) 对 \(\mathcal{H}\) 的变体生成。

3. 达尔文式演进层（选择与遗传）
- 通过真实评估压力筛掉伪收益，保留可复用有效结构。
- 在本系统中，对应 train + holdout 接受准则与向 \(\mathcal{M}\) 的保留写入。

三者可收敛为最小可执行三元组：

\[
约束 + 变体 + 选择
\]

这就是本文 MSES 闭环的操作化定义。

## 4.3 最小自我演进系统（MSES）

定义系统状态：

\[
S=\langle G,\mathcal{H},\mathcal{M},\mathcal{E},\mathcal{J},\Pi\rangle
\]

其中：

- \(G\)：目标函数（例如准确率、稳定性、代价约束）
- \(\mathcal{H}\)：候选策略空间
- \(\mathcal{M}\)：可持久化记忆
- \(\mathcal{E}\)：环境与任务分布
- \(\mathcal{J}\)：评估器
- \(\Pi\)：硬约束集合

演进接受准则：

\[
accept(h)=\left(\Delta train>0\right)\land\left(\Delta holdout>0\right)\land\left(score_{eff}>score_{parent}\right)\land\Pi
\]

其中：

\[
score_{eff}=score_{holdout}-\lambda\cdot cost-\mu\cdot risk
\]

该准则直接对应代码中的 `select_candidate` 和 `EvolutionRunner`。

## 5. 代码落地映射

### 5.1 演进决策核心

- `crates/prx-memory-core/src/mses.rs`
  - `select_candidate(...)`
  - train + holdout 双改进检查
  - 成本/风险惩罚
- `crates/prx-memory-core/src/evolution.rs`
  - `EvolutionRunner::run_generation(...)`
  - 在可接受候选中选取最高有效分数

### 5.2 MCP 暴露

- `crates/prx-memory-mcp/src/server.rs`
  - `exec_memory_evolve(...)`
  - 将演进决策对外暴露为 `memory_evolve` 工具

### 5.3 保持长期稳定的维护机制

- `crates/prx-memory-mcp/src/server.rs`
  - 每 100 次写入触发 `run_periodic_maintenance(...)`
  - 去重合并、decision 比例再平衡、低价值项清理

### 5.4 回归证据

- `crates/prx-memory-ai/tests/holdout_regression.rs`
  - 读取 `data/holdout/evolution_cases.json` 验证演进接受行为

## 6. 可行性论证

## 6.1 理论可行性

若演进必须同时满足 train 与 holdout 改进，且受约束门控，则系统可避免大多数过拟合型伪收益。该结论来自演进接受条件本身，具有可证伪性：任意不满足条件的候选都被拒绝。

## 6.2 工程可行性

`prx-memory` 已具备以下可运行基线：

1. 多传输：stdio + HTTP。
2. 多客户端验证脚本：`./scripts/run_multi_client_validation.sh`。
3. 工具回归：`cargo test -p prx-memory-mcp --all-targets --all-features`。
4. holdout 回归：`./scripts/run_holdout_regression.sh`。
5. 维护工具链：export/import/migrate/reembed/compact。

## 6.3 生产可行性

系统通过以下机制降低上线风险：

1. 配置化 provider 与降级路径。
2. 指标与告警（metrics + summary）。
3. 决策比例控制（decision <= 30%）。
4. 周期压缩和去重，抑制记忆膨胀。

## 7. 实践路径：从零配置到长期治理

为兼顾易用性与严谨性，系统提供 profile 化标准模式：

1. `zero-config`：低门槛接入，自动补全标准标签与默认策略。
2. `governed`：严格治理，强调双层写入和强约束。

关键环境变量：

- `PRX_MEMORY_STANDARD_PROFILE=zero-config|governed`
- `PRX_MEMORY_DEFAULT_PROJECT_TAG`
- `PRX_MEMORY_DEFAULT_TOOL_TAG`
- `PRX_MEMORY_DEFAULT_DOMAIN_TAG`

## 8. 与传统 memory 方案的区别

1. 不是单纯向量检索，而是“治理 + 检索 + 演进”联合系统。
2. 不是单客户端插件，而是 MCP 统一契约组件。
3. 不是只给建议，而是将规则编码为可执行工具约束。

## 9. 限制与威胁

1. 演进质量依赖评估器稳定性；若任务分布漂移，holdout 需更新。
2. 远端 embedding/rerank 依赖外部密钥与服务可用性。
3. 当前真实线上长期 A/B 数据仍需在更大规模任务集积累。

## 10. 结论

`prx-memory` 的核心价值不在“记住更多”，而在“记住可复用、可验证、可演进的知识”。通过将治理规则、检索能力和演进准则编码进同一执行系统，项目把 `need.md` 中的经验性要求转化为可运行、可测试、可审计的工程机制。这使“自我进化”从叙事变为可验证流程，为长期 Agent memory 提供现实可行的生产路径。

## 附录 A：复现实验命令

```bash
cargo test -p prx-memory-mcp --all-targets --all-features
./scripts/run_multi_client_validation.sh
./scripts/run_holdout_regression.sh
```

## 附录 B：建议引用文件

- `README.md`
- `skills/README.md`
- `docs/engineering/INSTALL_AND_TROUBLESHOOTING.md`
- `docs/engineering/OBSERVABILITY.md`
- `docs/ROADMAP.md`
