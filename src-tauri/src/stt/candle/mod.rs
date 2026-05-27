pub mod whisper;
pub mod wav2vec;

use candle_core::Device;

pub fn get_device(use_gpu: bool) -> Result<Device, candle_core::Error> {
    if use_gpu {
        #[cfg(target_os = "macos")]
        {
            Device::new_metal(0)
        }
        #[cfg(not(target_os = "macos"))]
        {
            #[cfg(feature = "cuda")]
            {
                Device::new_cuda(0)
            }
            #[cfg(not(feature = "cuda"))]
            {
                Ok(Device::Cpu)
            }
        }
    } else {
        Ok(Device::Cpu)
    }
}
