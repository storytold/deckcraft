//! wgpu instance configuration, applied before wgpu can initialize any driver.

#[cfg(any(target_os = "windows", test))]
use eframe::wgpu::{Backends, Dx12Compiler};
use eframe::{NativeOptions, egui_wgpu::WgpuSetup, wgpu::InstanceFlags};

/// wgpu 30 compiles its indirect-call validation compute pipelines while creating the device.
/// Metal's compiler fails on them on old Intel Macs (HD 4000 / Iris Pro, GPU family Mac1:
/// "Compiler encountered an internal error"), the device is lost and DeckCraft exits before its
/// window opens (#2146). Only Intel Macs skip them; every other platform keeps wgpu's default.
/// DeckCraft draws slides on the CPU and egui-wgpu issues no indirect draws or dispatches, so
/// the validation has nothing to check there.
fn skips_indirect_validation(os: &str, arch: &str) -> bool {
    os == "macos" && arch == "x86_64"
}

/// Leave out wgpu's indirect-call validation on Intel Macs (see [`skips_indirect_validation`]).
/// `WGPU_VALIDATION_INDIRECT_CALL` still overrides the choice.
pub fn configure_indirect_validation(options: &mut NativeOptions) {
    apply_indirect_validation(options, std::env::consts::OS, std::env::consts::ARCH);
}

fn apply_indirect_validation(options: &mut NativeOptions, os: &str, arch: &str) {
    if !skips_indirect_validation(os, arch) {
        return;
    }
    if let WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup {
        let flags = &mut create.instance_descriptor.flags;
        *flags = flags.difference(InstanceFlags::VALIDATION_INDIRECT_CALL).with_env();
        log::info!("Intel Mac graphics: instance flags={flags:?}");
    }
}

/// Default to DirectX 12 with FXC before creating the wgpu instance. Explicit
/// `WGPU_BACKEND` and `WGPU_DX12_COMPILER` choices, read by the caller, take precedence.
#[cfg(any(target_os = "windows", test))]
pub fn configure(options: &mut NativeOptions, backend_override: Option<Backends>, compiler_override: Option<Dx12Compiler>) {
    if let WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup {
        // eframe's default includes GL. Creating that backend can crash inside AMD's
        // atio6axx.dll before adapter selection or any Rust error handling runs, so the
        // window never appears. Choosing an adapter afterwards is too late: only leaving GL
        // out of the instance avoids it. This matches the verified WGPU_BACKEND=dx12
        // workaround without mutating process environment variables.
        create.instance_descriptor.backends = backend_override.unwrap_or(Backends::DX12);
        // Auto loads whichever dxcompiler.dll the DLL search path finds; DeckCraft ships
        // none, and without a matching dxil.dll the unsigned-DXIL warning fails wgpu's
        // indirect-validation pipeline and loses the device before the first frame
        // (issue #153). FXC uses the compiler that is part of Windows.
        create.instance_descriptor.backend_options.dx12.shader_compiler = compiler_override.unwrap_or(Dx12Compiler::Fxc);
        log::info!(
            "Windows graphics: backends={:?}, DX12 compiler={:?}",
            create.instance_descriptor.backends,
            create.instance_descriptor.backend_options.dx12.shader_compiler
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configured_backends(backend_override: Option<Backends>) -> Backends {
        let mut options = NativeOptions::default();
        configure(&mut options, backend_override, None);
        let WgpuSetup::CreateNew(create) = options.wgpu_options.wgpu_setup else {
            panic!("default native options must create a wgpu instance");
        };
        create.instance_descriptor.backends
    }

    #[test]
    fn windows_default_initializes_only_dx12() {
        // eframe's own default (PRIMARY | GL) would initialize OpenGL as well.
        let WgpuSetup::CreateNew(eframe_default) = NativeOptions::default().wgpu_options.wgpu_setup else {
            panic!("default native options must create a wgpu instance");
        };
        assert!(eframe_default.instance_descriptor.backends.contains(Backends::GL));
        assert_eq!(configured_backends(None), Backends::DX12);
    }

    #[test]
    fn windows_default_uses_fxc_instead_of_searching_for_dxc() {
        let mut options = NativeOptions::default();
        if let WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup {
            // Auto can find an unrelated DXC without its matching DXIL runtime (issue #153).
            create.instance_descriptor.backend_options.dx12.shader_compiler = Dx12Compiler::Auto;
        }
        configure(&mut options, None, None);
        let WgpuSetup::CreateNew(create) = options.wgpu_options.wgpu_setup else {
            panic!("default native options must create a wgpu instance");
        };
        assert!(matches!(create.instance_descriptor.backend_options.dx12.shader_compiler, Dx12Compiler::Fxc));
    }

    #[test]
    fn explicit_compiler_override_is_preserved() {
        for compiler in [Dx12Compiler::Fxc, Dx12Compiler::Auto, Dx12Compiler::DynamicDxc { dxc_path: "custom/dxcompiler.dll".into() }] {
            let mut options = NativeOptions::default();
            configure(&mut options, None, Some(compiler.clone()));
            let WgpuSetup::CreateNew(create) = options.wgpu_options.wgpu_setup else {
                panic!("default native options must create a wgpu instance");
            };
            match (compiler, create.instance_descriptor.backend_options.dx12.shader_compiler) {
                (Dx12Compiler::Fxc, Dx12Compiler::Fxc) | (Dx12Compiler::Auto, Dx12Compiler::Auto) => {}
                (Dx12Compiler::DynamicDxc { dxc_path: expected }, Dx12Compiler::DynamicDxc { dxc_path: actual }) => {
                    assert_eq!(actual, expected);
                }
                (expected, actual) => panic!("compiler override changed from {expected:?} to {actual:?}"),
            }
        }
    }

    #[test]
    fn explicit_backend_override_is_preserved() {
        for backends in [Backends::VULKAN, Backends::GL, Backends::DX12, Backends::VULKAN | Backends::DX12, Backends::empty()] {
            assert_eq!(configured_backends(Some(backends)), backends);
        }
    }

    fn instance_flags(options: &NativeOptions) -> InstanceFlags {
        let WgpuSetup::CreateNew(create) = &options.wgpu_options.wgpu_setup else {
            panic!("default native options must create a wgpu instance");
        };
        create.instance_descriptor.flags
    }

    #[test]
    fn only_intel_macs_skip_indirect_validation() {
        // Metal's compiler fails on the validation pipelines on HD 4000 / Iris Pro and the device is lost (#2146).
        assert!(skips_indirect_validation("macos", "x86_64"));
        // Apple silicon, Windows and Linux keep wgpu's validation.
        for (os, arch) in [("macos", "aarch64"), ("windows", "x86_64"), ("windows", "aarch64"), ("linux", "x86_64"), ("linux", "aarch64")] {
            assert!(!skips_indirect_validation(os, arch), "{os} {arch}");
        }
    }

    #[test]
    fn intel_mac_instance_drops_indirect_validation_and_others_keep_it() {
        let before = instance_flags(&NativeOptions::default());
        for (os, arch) in [("macos", "aarch64"), ("windows", "x86_64"), ("linux", "x86_64")] {
            let mut options = NativeOptions::default();
            apply_indirect_validation(&mut options, os, arch);
            assert_eq!(instance_flags(&options), before, "{os} {arch}: flags untouched");
        }
        // An explicit WGPU_VALIDATION_INDIRECT_CALL decides on its own, so only check the default.
        if std::env::var_os("WGPU_VALIDATION_INDIRECT_CALL").is_none() {
            assert!(before.contains(InstanceFlags::VALIDATION_INDIRECT_CALL), "wgpu's default validates indirect calls");
            let mut options = NativeOptions::default();
            apply_indirect_validation(&mut options, "macos", "x86_64");
            let flags = instance_flags(&options);
            assert!(!flags.contains(InstanceFlags::VALIDATION_INDIRECT_CALL), "{flags:?}");
            assert_eq!(flags, before.difference(InstanceFlags::VALIDATION_INDIRECT_CALL), "other flags kept");
        }
    }
}
