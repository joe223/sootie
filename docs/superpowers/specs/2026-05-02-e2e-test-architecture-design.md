# E2E测试架构设计

## 目标

建立独立的端到端测试体系，实现：
- **功能覆盖**：每个MCP工具和用户场景都有端到端验证
- **代码覆盖**：使用覆盖率工具确保核心路径100%执行
- **黑盒视角**：测试不关心模块实现，只验证用户场景行为

## 设计决策

### 覆盖策略
功能覆盖（优先） + 代码覆盖（补充）

### 组织方式
独立 `tests/` 目录，不依赖 crate 内部结构

### 分类维度
按用户场景分类（端到端流程），而非工具或模块

### 环境管理
自包含测试环境，自动启动依赖（Chrome、HTTP服务器、MCP服务器）

### Fixtures管理
集中 `tests/fixtures/` 目录，统一管理测试数据

## 目录结构

```
tests/                          # 独立E2E测试crate
├── Cargo.toml                  # 测试crate配置
├── src/
│   ├── lib.rs                  # 测试辅助库
│   ├── test_env.rs             # 自动化环境管理
│   ├── fixtures.rs             # fixtures加载工具
│   ├── assertions.rs           # 自定义断言
│   └── mocks.rs                # Mock服务器
│
├── browser-automation/         # 浏览器自动化场景
│   ├── form_submission.rs
│   ├── navigation.rs
│   ├── multi_tab.rs
│   └── canvas_interaction.rs
│
├── desktop-automation/         # 桌面自动化场景
│   ├── app_launch.rs
│   ├── window_management.rs
│   ├── cross_app.rs
│   └── accessibility_tree.rs
│
├── visual-fallback/            # 视觉fallback场景
│   ├── onnx_inference.rs
│   ├── coordinate_detection.rs
│   └── resolution_scaling.rs
│
├── error-recovery/             # 错误恢复场景
│   ├── browser_crash.rs
│   ├── network_failure.rs
│   ├── permission_denied.rs
│   └── concurrent_conflict.rs
│
├── protocol-compliance/        # MCP协议合规场景
│   ├── handshake.rs
│   ├── tool_discovery.rs
│   ├── error_responses.rs
│   └── jsonrpc_spec.rs
│
└── fixtures/                   # 集中fixtures目录
    ├── screenshots/            # 测试截图PNG
    ├── html-pages/             # 测试HTML页面
    ├── models/                 # ONNX测试模型
    ├── configs/                # 测试配置JSON
    └── expected-results/       # 预期结果JSON
```

## 核心组件

### 1. TestEnv - 自动化环境管理

```rust
pub struct TestEnv {
    chrome: Option<ChromeProcess>,
    http_server: Option<HttpServer>,
    mcp_server: Option<McpServer>,
}

impl TestEnv {
    pub fn launch() -> Result<Self>;
    pub fn launch_chrome(port: u16) -> Result<ChromeProcess>;
    pub fn launch_http_server(fixture_dir: PathBuf) -> Result<HttpServer>;
    pub fn health_check(&self) -> Result<()>;
}
```

**职责**：
- 自动启动Chrome（headless模式，随机端口）
- 启动本地HTTP服务器提供测试页面
- 启动MCP服务器接收工具调用
- 自动清理（Drop trait保证）

### 2. FixturesLoader - 测试数据加载

```rust
pub struct FixturesLoader;

impl FixturesLoader {
    pub fn load_screenshot(name: &str) -> Result<ScreenshotData>;
    pub fn load_html_page(name: &str) -> Result<String>;
    pub fn load_expected_result(name: &str) -> Result<serde_json::Value>;
    pub fn load_test_model() -> Result<PathBuf>;
}
```

**职责**：
- 从 `fixtures/` 目录加载测试数据
- 缓存已加载fixtures减少IO
- 验证fixtures完整性（缺少文件报错）

### 3. 自定义断言

```rust
pub mod assertions {
    pub fn assert_coordinate_in_bounds(coord: Coordinate, bounds: Bounds);
    pub fn assert_screenshot_similarity(actual: &[u8], expected: &[u8], threshold: f32);
    pub fn assert_jsonrpc_compliant(response: &JsonRpcResponse);
    pub fn assert_tool_success(response: &JsonRpcResponse);
    pub fn assert_tool_error(response: &JsonRpcResponse, expected_code: i32);
}
```

**职责**：
- 验证坐标在目标范围内
- 验证截图相似度（允许像素差异）
- 验证MCP响应符合JSON-RPC规范
- 验证工具调用成功/失败

## 测试场景设计

### 场景1：浏览器自动化 - 表单提交

**测试文件**：`browser-automation/form_submission.rs`

**测试用例**：
1. `test_fill_and_submit_form_successfully`
   - 启动Chrome打开表单页面
   - MCP调用：find → type → click → wait_for_element
   - 验证成功消息显示
   - 截图对比验证

2. `test_form_validation_error`
   - 空表单提交
   - 验证错误消息显示

3. `test_form_multi_field_workflow`
   - 多字段顺序填写
   - Tab键焦点切换

**覆盖工具**：sootie_find, sootie_type, sootie_click, sootie_wait, sootie_screenshot

**覆盖代码**：cdp.rs, perception.rs

### 场景2：视觉Fallback - ONNX推理

**测试文件**：`visual-fallback/onnx_inference.rs`

**测试用例**：
1. `test_onnx_model_detects_button_coordinates`
   - 加载小型测试ONNX模型
   - 加载测试截图
   - 执行推理获取坐标
   - 验证坐标准确性（误差<10像素）

2. `test_onnx_handles_different_resolutions`
   - 不同分辨率截图测试
   - 验证归一化处理正确

**覆盖代码**：vision.rs, LocalModelProvider

### 场景3：协议合规 - MCP握手

**测试文件**：`protocol-compliance/handshake.rs`

**测试用例**：
1. `test_mcp_handshake_success`
   - 发送initialize请求
   - 验证响应包含capabilities
   - 发送initialized通知

2. `test_mcp_handshake_invalid_version`
   - 发送错误协议版本
   - 验证错误响应

**覆盖代码**：server.rs, protocol.rs

## 代码覆盖率集成

### 工具选择
`cargo-llvm-cov`（推荐）或 `cargo-tarpaulin`

### 运行方式
```bash
# 安装工具
cargo install cargo-llvm-cov

# 运行覆盖率测试
cargo llvm-cov --workspace --html

# 检查覆盖率阈值
cargo llvm-cov --workspace --fail-under-lines 80
```

### 覆盖率目标
- Statements: 80%+
- Branches: 70%+
- Functions: 90%+

## 功能覆盖矩阵

| 场景分类 | 测试用例数 | 覆盖的MCP工具 | 覆盖的代码模块 |
|---------|-----------|-------------|--------------|
| 浏览器自动化 | 7 | find, type, click, wait, screenshot, press, hotkey | cdp.rs, perception.rs, action.rs |
| 桌面自动化 | 9 | launch, focus, window_move, window_resize, context, inspect | action.rs, perception.rs (macos/linux/windows) |
| 视觉Fallback | 6 | vision.detect() | vision.rs, local_model.rs |
| 错误恢复 | 8 | 所有CDP/MCP工具 | cdp.rs, server.rs (错误处理路径) |
| 协议合规 | 6 | initialize, tools/list | server.rs, protocol.rs |

**总计**：约36个端到端测试场景，覆盖19个MCP工具

## 实施计划

### Phase 1：基础设施（1-2天）
1. 创建 `tests/` 目录和 Cargo.toml
2. 实现 TestEnv 自动化环境管理
3. 实现 FixturesLoader
4. 实现自定义断言
5. 创建基础fixtures（HTML页面、配置文件）

### Phase 2：浏览器自动化测试（2-3天）
1. form_submission.rs（3个测试）
2. navigation.rs（2个测试）
3. multi_tab.rs（2个测试）

### Phase 3：桌面自动化测试（2-3天）
1. app_launch.rs（3个测试）
2. window_management.rs（4个测试）
3. accessibility_tree.rs（2个测试）

### Phase 4：视觉Fallback测试（1-2天）
1. onnx_inference.rs（3个测试）
2. coordinate_detection.rs（3个测试）

### Phase 5：错误恢复和协议合规（1-2天）
1. error-recovery（8个测试）
2. protocol-compliance（6个测试）

### Phase 6：覆盖率验证（1天）
1. 配置覆盖率工具
2. 运行覆盖率报告
3. 补充遗漏测试

## 测试运行方式

### 本地开发
```bash
# 运行所有E2E测试
cargo test --test e2e

# 运行特定场景
cargo test --test browser-automation

# 运行单个测试
cargo test --test form_submission test_fill_and_submit_form_successfully
```

### CI集成
```yaml
# .github/workflows/test.yml
- name: Run E2E tests
  run: cargo test --workspace
  
- name: Generate coverage report
  run: cargo llvm-cov --workspace --html
  
- name: Upload coverage
  uses: codecov/codecov-action@v3
```

## 技术约束

### 端口管理
- Chrome调试端口：动态分配（9222-9230范围）
- HTTP服务器端口：动态分配（8080-8090范围）
- 避免端口冲突，支持并行测试

### 资源清理
- Drop trait保证进程清理
- 测试失败也要清理环境
- 临时文件自动删除

### 测试隔离
- 每个测试独立环境实例
- 不共享Chrome实例（避免状态污染）
- fixtures只读，不修改

## 成功标准

1. **功能覆盖**：所有19个MCP工具至少有1个成功场景测试 + 1个错误场景测试
2. **代码覆盖**：核心模块（cdp.rs, vision.rs, server.rs）覆盖率 > 80%
3. **测试稳定性**：连续运行10次，成功率 > 95%
4. **执行速度**：单个测试 < 5秒，全部测试 < 2分钟
5. **CI就绪**：GitHub Actions自动运行，覆盖率报告上传

## 风险与缓解

| 风险 | 影响 | 缓解措施 |
|-----|------|---------|
| Chrome启动失败 | 测试无法运行 | 健康检查 + 重试机制 + 环境检测提示 |
| ONNX模型文件缺失 | 视觉测试失败 | 提供mini测试模型 + CI下载脚本 |
| 截图对比差异大 | 断言失败 | 允许阈值配置 + 忽略无关像素 |
| 并发测试端口冲突 | 测试失败 | 动态端口分配 + 端口范围管理 |
| macOS/Linux/Windows差异 | 跨平台失败 | 平台特定fixtures + 条件编译测试 |

## 后续优化

1. **性能测试**：添加响应时间断言
2. **压力测试**：并发100个工具调用
3. **兼容性测试**：Chrome/Firefox/Safari不同版本
4. **回归测试**：每日自动运行，失败自动告警