#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
#![forbid(unsafe_code)]
use clap::Parser;
use gliff_desktop::{config::Options, ui::Desktop};

fn main() -> eframe::Result {
    let options = Options::parse();
    let native = native_options();
    #[cfg(windows)]
    let (native, keyboard) =
        gliff_desktop::physical_keyboard::win32::NativeKeyboard::install(native);
    eframe::run_native(
        "Gliff",
        native,
        Box::new(move |_context| {
            #[cfg(windows)]
            keyboard.bind(_context)?;
            Ok(Box::new(Desktop::new(
                options,
                #[cfg(windows)]
                keyboard,
            )))
        }),
    )
}

fn native_options() -> eframe::NativeOptions {
    use eframe::{
        egui_wgpu::{WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew},
        wgpu,
    };
    let mut setup = WgpuSetupCreateNew::default();
    #[cfg(windows)]
    {
        // D3D12 includes Microsoft's WARP software adapter on Windows 11.
        // Vulkan remains useful on devices with vendor graphics drivers.
        setup.instance_descriptor.backends = wgpu::Backends::DX12 | wgpu::Backends::VULKAN;
    }
    // Prefer hardware, but keep CPU adapters eligible. GLIFF_SOFTWARE_RENDERER
    // is useful on VMs whose virtual GPU advertises incomplete D3D support.
    let software = std::env::var_os("GLIFF_SOFTWARE_RENDERER").is_some_and(|v| v == "1");
    setup.native_adapter_selector = Some(std::sync::Arc::new(move |adapters, surface| {
        adapters.iter().filter(|a| surface.is_none_or(|s| a.is_surface_supported(s)))
            .filter(|a| !software || a.get_info().device_type == wgpu::DeviceType::Cpu)
            .min_by_key(|a| match a.get_info().device_type {
                wgpu::DeviceType::DiscreteGpu => 0, wgpu::DeviceType::IntegratedGpu => 1,
                wgpu::DeviceType::VirtualGpu => 2, wgpu::DeviceType::Other => 3, wgpu::DeviceType::Cpu => 4,
            }).cloned().ok_or_else(|| "No compatible graphics adapter. Update the display driver or try GLIFF_SOFTWARE_RENDERER=1 for an available software adapter.".into())
    }));
    eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([640.0, 400.0]),
        wgpu_options: WgpuConfiguration {
            wgpu_setup: WgpuSetup::CreateNew(setup),
            ..Default::default()
        },
        ..Default::default()
    }
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn windows_renderer_has_a_compiled_direct3d_backend() {
        assert!(eframe::wgpu::Instance::enabled_backend_features()
            .contains(eframe::wgpu::Backends::DX12));
    }
}
