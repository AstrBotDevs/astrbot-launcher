use super::types::ComponentId;
use super::{nodejs, python, uv};

#[derive(Clone, Copy)]
pub struct ComponentDescriptor {
    pub id: ComponentId,
    pub description: &'static str,
    pub is_installed: fn() -> bool,
}

pub const COMPONENT_DESCRIPTORS: [ComponentDescriptor; 3] = [
    ComponentDescriptor {
        id: ComponentId::Python,
        description: "Python 3.10 / 3.12 运行时",
        is_installed: python::is_component_installed,
    },
    ComponentDescriptor {
        id: ComponentId::Nodejs,
        description: "Node.js 运行时",
        is_installed: nodejs::is_nodejs_installed,
    },
    ComponentDescriptor {
        id: ComponentId::UV,
        description: "uv / uvx 包管理工具",
        is_installed: uv::is_uv_installed,
    },
];
