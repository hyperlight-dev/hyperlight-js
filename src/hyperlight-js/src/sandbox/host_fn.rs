use std::collections::HashMap;

use serde::de::DeserializeOwned;
use serde::ser::SerializeSeq;
use serde::Serialize;

// Unlike hyperlight-host's Function, this Function trait uses `serde`'s Serialize and DeserializeOwned traits for input and output types.

/// A trait representing a host function that can be called from the guest JavaScript code.
///
/// This trait lets us workaround the lack of variadic generics in Rust by defining implementations
/// for tuples of different sizes.
/// The `call` method takes a single argument of type `Args`, which is expected to be a tuple
/// containing all the arguments for the function, and spreads them to the arguments n-arity when calling
/// the underlying function.
///
/// This trait has a blanket implementation for any function that takes arguments that are serde deserializable,
/// and return a serde serializable result, so you would never need to implement this trait directly.
pub trait Function<Output: Serialize, Args: DeserializeOwned> {
    fn call(&self, args: Args) -> Output;
}

// This blanket implementation allows us to implement the `Function` trait for any function that takes
// arguments that are serde deserializable, and return a serde serializable result.
impl<Output, Args, F> Function<Output, Args> for F
where
    Output: Serialize,
    Args: DeserializeOwned,
    F: fn_traits::Fn<Args, Output = Output>,
{
    fn call(&self, args: Args) -> Output {
        F::call(self, args)
    }
}

type BoxFunction = Box<dyn Fn(String) -> crate::Result<String> + Send + Sync>;

fn type_erased<Output: Serialize, Args: DeserializeOwned>(
    func: impl Function<Output, Args> + Send + Sync + 'static,
) -> BoxFunction {
    Box::new(move |args: String| {
        let args: Args = serde_json::from_str(&args)?;
        let output: Output = func.call(args);
        Ok(serde_json::to_string(&output)?)
    })
}

/// A module containing host functions that can be called from the guest JavaScript code.
#[derive(Default)]
pub struct HostModule {
    functions: HashMap<String, BoxFunction>,
}

// The serialization of this struct has to match the deserialization in
// register_host_modules in src/hyperlight-js-runtime/src/main/hyperlight.rs
impl Serialize for HostModule {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq_serializer = serializer.serialize_seq(Some(self.functions.len()))?;
        for key in self.functions.keys() {
            seq_serializer.serialize_element(key)?;
        }
        seq_serializer.end()
    }
}

impl HostModule {
    /// Register a host function that can be called from the guest JavaScript code.
    ///
    /// Registering a function with the same `name` as an existing function
    /// overwrites the previous registration.
    pub fn register<Output: Serialize, Args: DeserializeOwned>(
        &mut self,
        name: impl Into<String>,
        func: impl Function<Output, Args> + Send + Sync + 'static,
    ) -> &mut Self {
        self.functions.insert(name.into(), type_erased(func));
        self
    }

    pub(crate) fn get(&self, name: &str) -> Option<&BoxFunction> {
        self.functions.get(name)
    }
}
