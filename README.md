![Logo](https://github.com/user-attachments/assets/d28490ba-17f7-4c44-a27c-a9bcb22dd036)

# AstrBot Launcher

AstrBot Launcher是一款用于图形化管理AstrBot的桌面应用程序，提供版本下载、实例管理、数据备份以及Python运行环境自动化配置等完整功能支持。

## 功能特性

- 零侵入架构设计：运行环境与依赖统一在独立目录管理，避免污染系统
- 多实例可视化管理：创建/启动/停止/升级一站式完成
- 安全备份恢复：实例级备份与恢复，数据更安心
- 运行时隔离：实例独立运行，杜绝环境冲突
- 桌面友好集成：托盘驻留、开机自启即装即用

## 平台支持级别

| 操作系统 | 支持级别 | 说明 |
| :--- | :--- | :--- |
| Windows | 主要支持 | 主力开发与测试平台，功能完整，优先修复问题 |
| Linux | 尽力而为 | 提供可用支持，但不同桌面环境存在兼容性差异 |
| macOS | 理论上可工作 | 代码设计上兼容，但当前缺少充分实机验证 |

## 关于 Windows ARM 设备的兼容性说明

在Windows ARM设备上，启动器会统一使用x86_64版本的Python运行时（通过系统仿真层运行），以确保与现有AstrBot版本的兼容性。

## FAQ

如果遇到以下问题，可以按照对应步骤进行故障排除。

> [!important]
> 请确保已升级到最新版本。

### 下载太慢/网络错误

请点击软件页面最左边的“高级”并按需配置代理或源。如果对“代理”、“源”等概念感到陌生，打开“中国大陆一键加速”通常能够解决绝大部分问题。

![Mainland Acceleration](https://pic1.imgdb.cn/item/69b276c8cda91d5fbafff6d8.png)

### DLL加载失败（常见于Windows ARM64）

```text
ValueError: the greenlet library is required to use this function.  
DLL load failed while importing _greenlet: The specified module could not be found.
```

**解决方案：**

> [!note]
> Windows ARM64也是安装下面的运行库，因为它同时包含了ARM64和X64二进制文件。

请下载安装Microsoft Visual C++ Redistributable：

点此链接下载： [https://aka.ms/vc14/vc_redist.x64.exe](https://aka.ms/vc14/vc_redist.x64.exe)

### 依赖同步失败

```text
uv sync failed  
Failed to uninstall package ...  
Installation may result in an incomplete environment  
missing RECORD file
```

**解决方案：**

1. 点击软件页面最左边的“高级”
2. 向下滚动至故障排除
3. 选择对应实例
4. 点击执行清空虚拟环境

### OS error 5 拒绝访问

```text
OS error 5  
拒绝访问
Access is denied
```

**解决方案：**

该问题通常情况下是文件被其他进程占用所致，可尝试以下方法解决：

- 关闭正在占用文件的程序
- 使用解除占用工具
- 直接重启电脑

常见的解除占用工具：

- 360、火绒等安全软件自带工具
- PowerToys 0.64及以上版本内置的Locksmith

鉴于本项目无法对其他第三方解锁工具的安全性作出保证，故不在此处列举更多相关软件。

## 中国大陆一键加速使用的镜像与服务

当用户在“高级设置 -> 通用”中开启“中国大陆一键加速”后，启动器会忽略手动填写的代理和源设置，并改用以下预设地址：

- npm 镜像源：<https://npmreg.proxy.ustclug.org/>
- Node.js 下载镜像：<https://mirrors.ustc.edu.cn/node/>
- PyPI 镜像源：<https://mirrors.ustc.edu.cn/pypi/>
- `astral-sh/python-build-standalone` 发布镜像：<https://mirrors.ustc.edu.cn/github-release/astral-sh/python-build-standalone/LatestRelease/>
- `astral-sh/uv` 发布镜像：<https://mirrors.ustc.edu.cn/github-release/astral-sh/uv/LatestRelease/>
- AstrBot Release 下载加速代理： <https://gh-proxy.com>

感谢上述镜像与服务的维护者持续向公众提供可用的基础设施与网络服务，为开源软件分发、依赖安装和版本获取带来了切实帮助。

> [!note]
> 如果相关镜像源或服务的维护者认为本项目不应内置上述某个地址，请提出Issue并明确说明诉求，我们会及时进行处理。

## 技术栈

- 前端: React 19, Vite, Ant Design, TypeScript
- 后端: Rust + Tauri 2

## 安全性说明

本项目所有源代码公开，内嵌二进制文件ctrlc_sender.exe源码托管于<https://codeberg.org/Raven95676/ctrlc_sender>

## 附注

如果本项目对您的生活/工作产生帮助，请给项目一个 Star ❤️
