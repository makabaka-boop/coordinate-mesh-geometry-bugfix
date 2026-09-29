# meshcheck — 三角面片拓扑与几何审计器

一个零依赖的 Rust 命令行工具，读取**顶点 id** 与**有向三角面**列表，
证明输入是否构成一张**组合拓扑水密**（combinatorial watertight）的
可定向封闭三角曲面。

无坐标文档继续执行组合拓扑审计；每个顶点都附带整数坐标的文档，
在拓扑通过后还要审计三角面的几何嵌入。

## 审计的问题

每条无向边恰好出现两次，并不足以保证曲面可靠封闭——两个封闭壳
只在一个顶点上相碰时，所有边依然两两反向配对，但该顶点处会
**捏成两个互不相连的扇区**。因此审计按**固定优先级**分三级进行：

1. **边级（edge）**：每条无向边必须恰被两个面使用，且两面以
   **相反方向**经过它。
   - 仅出现一次 → `boundary`（边界边）
   - 出现两次但同向 → `misoriented`（翻面）
   - 出现 ≥ 3 次 → `nonmanifold`（非流形边）
   - 见证（witness）取字典序最小的故障无向边
     `(min(id1,id2), max(id1,id2))`。

2. **顶点级（vertex sectors）**：每个顶点周围的面必须连成**唯一一个
   循环扇区**，即该顶点的链接（link）是单一有向环。对有向面
   `(x,y,z)`，在顶点 `v` 处的链接边从 `v` 的后继指向前驱；共享边
   把相邻面的链接端点粘起来，链接的弱连通分量数就是 `v` 处的
   扇区数。扇区数 ≠ 1（如双壳共点 = 2，孤立声明顶点 = 0）即失败，
   见证取字典序最小的顶点 id 及其扇区数。

3. **整体连通（component）**：所有面必须处于同一个共边连通分量。
   见证为**不含全局最小顶点的分量中字典序最小的顶点 id**。

带坐标文档另有几何审计：零面积面不合法；两个不同面只能沿其
共同的顶点或共同的完整边相接，其他交叉、贴碰和共面重叠都不合法。
先按输入顺序报告最早的零面积面；若无此类面，再按面序对报告最早
发生非法接触的一对面。面序从 1 开始。无坐标文档跳过这一级。

全部通过后输出 `V`、`E`、`F`、欧拉示性数 `χ = V − E + F` 与
整数亏格 `g = (2 − χ)/2`。

## 输入格式

UTF-8 ASCII 文本，每行一条记录，`#` 之后为行内注释，空行允许：

```text
v <id>                    # 声明一个顶点
v <id> <x> <y> <z>        # 或在所有顶点上同时给出整数坐标
f <id> <id> <id>          # 一个有向三角面（按面的一致绕序书写）
```

- 顶点 id：1–32 个字符，字符集 `[A-Za-z0-9_-]`。
- 坐标为 -10000 到 10000 的十进制整数；有坐标与无坐标的顶点记录不能混用。
- 规模限定：**4–500** 个唯一顶点，**4–2000** 个面。
- 硬性拒绝（在拓扑审计之前，不进入三级流程）：
  无法识别的行、顶点/面记录字段数错误、非法 id、面内三顶点不互异、
  顶点重复声明、面引用未知顶点、**重复无向面（即使第二份反向书写）**。

## 输出与退出码

```text
ok V=4 E=6 F=4 chi=2 genus=0
edge-fail edge=a-b uses=1 fault=boundary
edge-fail edge=a-b uses=2 fault=misoriented
edge-fail edge=a-b uses=6 fault=nonmanifold
vertex-fail vertex=a sectors=2
component-fail vertex=e
reject kind=unknown_vertex line=9 id=x
reject kind=duplicate_face line=11
reject kind=bad_face_arity line=8
geometry-fail kind=degenerate face=3
geometry-fail kind=intersection faces=3,7
```

| 退出码 | 含义                                 |
| ------ | ------------------------------------ |
| 0      | 封闭可定向曲面，已给出 V/E/F/χ/亏格  |
| 1      | 输入被拒绝（语法/引用/重复面/越界）  |
| 2      | 边级失败                             |
| 3      | 顶点扇区失败（也用于 `--help`）      |
| 4      | 整体不连通                           |
| 5      | 几何嵌入失败                         |
| 3      | 用法/IO 错误（多参数、文件不可读等） |

## 使用

### Cargo

```bash
cargo build --release
cargo test                                        # 证据与几何一致性测试
./target/release/meshcheck examples/torus9.mesh   # 文件参数
cat examples/tet.mesh | meshcheck                 # 无参数或 `-` 时读标准输入
```

### Docker Compose

`meshcheck` 服务默认对镜像内置的 9 顶点环面夹具做一次审计：

```bash
docker compose build
docker compose run --rm meshcheck                  # 默认审计 torus9
docker compose run --rm meshcheck /examples/tet.mesh
# 把待审计文件放进 ./work 后：
docker compose run --rm meshcheck /work/your-model.mesh
```

## 夹具（examples/）

| 文件                            | 预期结果                                   |
| ------------------------------- | ------------------------------------------ |
| `tet.mesh`                      | 封闭四面体，χ=2，亏格 0（合法封闭体）      |
| `torus9.mesh`                   | 9 顶点 18 面环面，χ=0，亏格 1（合法）      |
| `genus2.mesh`                   | 两个环面沿洞反向粘合（T#T），χ=−2，亏格 2  |
| `flipped.mesh`                  | 翻面 → 最小同向边 `a-b`                    |
| `boundary.mesh`                 | 五面体缺一个顶盖 → 边界边 `a-b`            |
| `nonmanifold-edge.mesh`         | 三个四面体共一条边 → 非流形边 `a-b`        |
| `two-shells-shared-vertex.mesh` | 两壳共一个顶点 → 顶点 `a` 两个扇区         |
| `disjoint-shells.mesh`          | 两个互不相连的封闭四面体 → 分量见证 `e`    |
| `tet-coord.mesh`                | 带整数坐标的封闭四面体 → 几何审计通过 `ok` |
| `octahedron-coord.mesh`         | 坐标八面体；相对面 AABB 重叠但平面平行不接触 → `ok` |
| `degenerate-face.mesh`          | 第 4 个面压成一条线 → `kind=degenerate face=4` |
| `self-overlap.mesh`            | 共边两面同侧重叠 → `kind=intersection faces=2,4` |
| `unknown-vertex.mesh`           | 引用未声明顶点                             |
| `duplicate-face.mesh`           | 无向面重复（第二份反向书写）               |
| `extra-field.mesh`              | 面记录含额外字段                           |

## 限制

- 输出的整数亏格仅在审计全通过时才有拓扑意义。
