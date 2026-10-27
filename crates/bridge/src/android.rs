//! Android-only JNI entry points, called from Kotlin before anything uses TLS.

use jni::EnvUnowned;
use jni::errors::ThrowRuntimeExAndDefault;
use jni::objects::{JClass, JObject};

/// `Bridge.initTls(context)` in Kotlin. Hands the app context to
/// `rustls-platform-verifier`, which calls Android's certificate verifier
/// through the JVM. Errors become a Java `RuntimeException`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_danielesc0911_lemusync_Bridge_initTls<'caller>(
    mut env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    context: JObject<'caller>,
) {
    env.with_env(|env| rustls_platform_verifier::android::init_with_env(env, context))
        .resolve::<ThrowRuntimeExAndDefault>();
}
