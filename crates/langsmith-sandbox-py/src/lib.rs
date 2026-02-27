use lsandbox::{RunOpts, SandboxClient, SandboxError, SandboxInfo};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict};
use pyo3_async_runtimes::tokio::future_into_py;

fn py_err(err: SandboxError) -> PyErr {
    PyRuntimeError::new_err(err.to_string())
}

fn sandbox_info_to_pydict(py: Python<'_>, info: &SandboxInfo) -> PyResult<Py<PyDict>> {
    let dict = PyDict::new(py);
    dict.set_item("name", &info.name)?;
    dict.set_item("template_name", &info.template_name)?;
    dict.set_item("dataplane_url", &info.dataplane_url)?;
    dict.set_item("id", &info.id)?;
    dict.set_item("created_at", &info.created_at)?;
    dict.set_item("updated_at", &info.updated_at)?;
    Ok(dict.unbind())
}

fn execution_result_to_pydict(
    py: Python<'_>,
    result: &lsandbox::ExecutionResult,
) -> PyResult<Py<PyDict>> {
    let dict = PyDict::new(py);
    dict.set_item("stdout", &result.stdout)?;
    dict.set_item("stderr", &result.stderr)?;
    dict.set_item("exit_code", result.exit_code)?;
    Ok(dict.unbind())
}

#[pyclass(name = "SandboxClient", unsendable)]
struct PySandboxClient {
    inner: SandboxClient,
}

#[pymethods]
impl PySandboxClient {
    #[new]
    #[pyo3(signature = (api_key=None, endpoint=None))]
    fn new(api_key: Option<String>, endpoint: Option<String>) -> PyResult<Self> {
        let resolved_key = match api_key {
            Some(k) => k,
            None => std::env::var("LANGSMITH_API_KEY").unwrap_or_default(),
        };
        if resolved_key.trim().is_empty() {
            return Err(PyValueError::new_err(
                "LANGSMITH_API_KEY is required (or pass api_key explicitly)",
            ));
        }

        let inner = match endpoint {
            Some(ep) if !ep.trim().is_empty() => {
                SandboxClient::new_with_endpoint(&resolved_key, &ep).map_err(py_err)?
            }
            _ => SandboxClient::new(&resolved_key).map_err(py_err)?,
        };

        Ok(Self { inner })
    }

    fn list_template_names<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            let templates = client.list_templates().await.map_err(py_err)?;
            Ok(templates.into_iter().map(|t| t.name).collect::<Vec<_>>())
        })
    }

    fn get_sandbox<'py>(&self, py: Python<'py>, name: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            let sandbox = client.get_sandbox(&name).await.map_err(py_err)?;
            Python::with_gil(|py| sandbox_info_to_pydict(py, &sandbox.info))
        })
    }

    #[pyo3(signature = (template_name, name=None))]
    fn create_sandbox<'py>(
        &self,
        py: Python<'py>,
        template_name: String,
        name: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            let sandbox = client
                .create_sandbox(&template_name, name.as_deref())
                .await
                .map_err(py_err)?;
            Python::with_gil(|py| sandbox_info_to_pydict(py, &sandbox.info))
        })
    }

    #[pyo3(signature = (sandbox_name, command, timeout=None, cwd=None, shell=None))]
    fn run<'py>(
        &self,
        py: Python<'py>,
        sandbox_name: String,
        command: String,
        timeout: Option<u64>,
        cwd: Option<String>,
        shell: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let mut opts = RunOpts::new(&command);
        if let Some(secs) = timeout {
            opts = opts.timeout(secs);
        }
        if let Some(dir) = cwd.as_deref() {
            opts = opts.cwd(dir);
        }
        if let Some(shell_path) = shell.as_deref() {
            opts = opts.shell(shell_path);
        }
        let client = self.inner.clone();
        future_into_py(py, async move {
            let sandbox = client.get_sandbox(&sandbox_name).await.map_err(py_err)?;
            let result = sandbox.run_with(&opts).await.map_err(py_err)?;
            Python::with_gil(|py| execution_result_to_pydict(py, &result))
        })
    }

    fn read_file<'py>(
        &self,
        py: Python<'py>,
        sandbox_name: String,
        path: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            let sandbox = client.get_sandbox(&sandbox_name).await.map_err(py_err)?;
            let bytes = sandbox.read(&path).await.map_err(py_err)?;
            Python::with_gil(|py| Ok(PyBytes::new(py, &bytes).unbind()))
        })
    }

    fn write_file<'py>(
        &self,
        py: Python<'py>,
        sandbox_name: String,
        path: String,
        content: Vec<u8>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            let sandbox = client.get_sandbox(&sandbox_name).await.map_err(py_err)?;
            sandbox.write(&path, &content).await.map_err(py_err)?;
            Python::with_gil(|py| Ok(py.None()))
        })
    }
}

#[pymodule]
fn lsandbox_py(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PySandboxClient>()?;
    Ok(())
}
