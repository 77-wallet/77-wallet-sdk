/// Expand Address System
///
/// 核心组件：
/// - ExpandPlanner: 补全缺失的ExpandBatchItem，遵循幂等边界和数量计算公式
/// - ExpandScanner: 定时扫描并推进状态，系统的核心驱动
/// - ExpandExecutor: 执行具体的create/init操作，无状态设计
/// - ExpandActor: 消息接收者，不再承担系统推进职责
///
/// 核心流程：
/// 1. Scanner 触发 Planner 补全缺失的 items
/// 2. Scanner 扫描并推进 ExpandBatchItem 状态
/// 3. Scanner 调用 Executor 执行具体操作
/// 4. 状态只能向前推进，失败不回退
/// 5. 数据库是唯一事实源
pub(crate) mod actor;
pub(crate) mod bootstrap;
pub(crate) mod event;
pub(crate) mod executor;
pub(crate) mod facade;
pub(crate) mod planner;
pub(crate) mod scanner;
pub(crate) mod service;
pub(crate) mod worker;
