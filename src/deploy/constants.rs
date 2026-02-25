// Allow dead code for now since this module is being ported but not yet used
#![allow(dead_code)]

pub const MIN_NODE_VERSION: u32 = 20;
pub const DEFAULT_NODE_VERSION: &str = "20";

pub const MIN_PYTHON_VERSION: (u32, u32) = (3, 11);
pub const DEFAULT_PYTHON_VERSION: &str = "3.11";

pub const DEFAULT_IMAGE_DISTRO: &str = "debian";

pub const BUILD_TOOLS: &[&str] = &["pip", "setuptools", "wheel"];

pub const DEFAULT_POSTGRES_URI: &str =
    "postgres://postgres:postgres@langgraph-postgres:5432/postgres?sslmode=disable";

pub const VALID_DISTROS: &[&str] = &["debian", "wolfi", "bookworm"];
pub const VALID_PIP_INSTALLERS: &[&str] = &["auto", "pip", "uv"];

pub const RESERVED_PACKAGE_NAMES: &[&str] = &[
    "src",
    "langgraph-api",
    "langgraph_api",
    "langgraph",
    "langchain-core",
    "langchain_core",
    "pydantic",
    "orjson",
    "fastapi",
    "uvicorn",
    "psycopg",
    "httpx",
    "langsmith",
];
