use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine as _;
use hmac::digest::{FixedOutputReset, KeyInit};
use hmac::Mac;
use rquickjs::class::Trace;
use rquickjs::{Ctx, Exception, JsLifetime, Result, Value};
use sha2::{Sha256, Sha384, Sha512};

use crate::utils::as_bytes;

#[rquickjs::module(rename_vars = "camelCase", rename_types = "camelCase")]
#[allow(clippy::module_inception)]
pub mod crypto {
    use super::*;

    #[rquickjs::function]
    pub fn create_hmac(ctx: Ctx<'_>, algo: String, key: Value<'_>) -> rquickjs::Result<Hmac> {
        Hmac::new(ctx, algo, key)
    }

    #[rquickjs::class()]
    #[derive(Clone, Trace, JsLifetime)]
    pub struct Hmac {
        #[qjs(skip_trace)]
        inner: Rc<RefCell<HmacInner>>,
    }

    #[rquickjs::methods]
    impl Hmac {
        #[qjs(constructor)]
        pub fn new(ctx: Ctx<'_>, algorithm: String, key: Value<'_>) -> rquickjs::Result<Self> {
            let key = as_bytes(key)?;
            let inner = match algorithm.to_lowercase().as_str() {
                "sha256" => HmacInner::with_key::<hmac::Hmac<Sha256>>(&ctx, key),
                "sha384" => HmacInner::with_key::<hmac::Hmac<Sha384>>(&ctx, key),
                "sha512" => HmacInner::with_key::<hmac::Hmac<Sha512>>(&ctx, key),
                _ => Err(Exception::throw_type(
                    &ctx,
                    &format!("Invalid algorithm: {algorithm:?}"),
                )),
            }?;
            Ok(Self { inner })
        }

        pub fn update(&mut self, data: Value<'_>) -> Result<Self> {
            self.inner.borrow_mut().update(data)?;
            Ok(self.clone())
        }

        pub fn finalize(&mut self) -> Self {
            self.inner.borrow_mut().finalize();
            self.clone()
        }

        pub fn digest(&mut self, ctx: Ctx<'_>, format: String) -> Result<String> {
            self.inner.borrow_mut().digest(ctx, format)
        }
    }
}

trait DynHmac {
    fn update(&mut self, data: &[u8]);
    fn finalize(&mut self) -> Vec<u8>;
}

impl<T: Mac + FixedOutputReset> DynHmac for T {
    fn update(&mut self, data: &[u8]) {
        Mac::update(self, data)
    }

    fn finalize(&mut self) -> Vec<u8> {
        Mac::finalize_reset(self).into_bytes().to_vec()
    }
}

struct HmacInner_<T: DynHmac + ?Sized> {
    result: Vec<u8>,
    hmac: T,
}

type HmacInner = HmacInner_<dyn DynHmac>;

impl HmacInner {
    fn with_key<T: DynHmac + KeyInit + 'static>(
        ctx: &Ctx<'_>,
        key: impl AsRef<[u8]>,
    ) -> rquickjs::Result<Rc<RefCell<Self>>> {
        let hmac = T::new_from_slice(key.as_ref()).map_err(|e| {
            rquickjs::Exception::throw_type(ctx, &format!("Invalid hmac key: {e:#?}"))
        })?;
        let result = Vec::new();
        Ok(Rc::new(RefCell::new(HmacInner_ { result, hmac })))
    }

    pub fn update(&mut self, data: Value<'_>) -> rquickjs::Result<&mut Self> {
        let data = as_bytes(data)?;
        if !self.result.is_empty() {
            self.result.clear();
        }
        self.hmac.update(&data);
        Ok(self)
    }

    pub fn finalize(&mut self) -> &mut Self {
        self.result = self.hmac.finalize();
        self
    }

    pub fn digest(&mut self, ctx: Ctx<'_>, format: String) -> rquickjs::Result<String> {
        if self.result.is_empty() {
            self.finalize();
        }
        match format.to_lowercase().as_str() {
            "base64" => Ok(STANDARD.encode(&self.result)),
            "base64url" => Ok(URL_SAFE_NO_PAD.encode(&self.result)),
            "hex" => Ok(hex::encode(&self.result)),
            _ => Err(Exception::throw_type(
                &ctx,
                &format!("Unsupported format: {format:?}"),
            )),
        }
    }
}
