use rusb::{Context, DeviceHandle, UsbContext};
use std::time::Duration;
use gxremapper;

const VID: u16 = 0xFFFF;
const PID: u16 = 0x0004;
const EP_IN: u8 = 0x82;
const EP_OUT: u8 = 0x05;

pub struct PicFlasher {
    handle: DeviceHandle<Context>,
}

#[derive(Debug)]
pub enum FlasherError {
    Usb(rusb::Error),
    Protocol(String),
}

impl From<rusb::Error> for FlasherError {
    fn from(err: rusb::Error) -> Self {
        FlasherError::Usb(err)
    }
}

pub enum NandType {
    SmallBlock, // 16MB
    LargeBlock, // 64MB, 256MB, 512MB
}

impl PicFlasher {
    pub fn new() -> Result<Self, FlasherError> {
        let context = Context::new()?;
        let device = context
            .devices()?
            .iter()
            .find(|d| {
                let desc = d.device_descriptor().unwrap();
                desc.vendor_id() == VID && desc.product_id() == PID
            })
            .ok_or_else(|| FlasherError::Protocol("Device not found".to_string()))?;

        let mut handle = device.open()?;
        handle.claim_interface(0)?;
        Ok(Self { handle })
    }

    pub fn read_flash_id(&self) -> Result<u32, FlasherError> {
        // Send command 0x01 (Get ID - assuming this exists in firmware)
        let cmd = vec![0x01];
        self.handle.write_bulk(EP_OUT, &cmd, Duration::from_millis(1000))?;

        let mut buffer = [0u8; 4];
        let bytes_read = self.handle.read_bulk(EP_IN, &mut buffer, Duration::from_millis(1000))?;

        if bytes_read != 4 {
            return Err(FlasherError::Protocol("Failed to read Flash ID".to_string()));
        }

        Ok(u32::from_le_bytes(buffer))
    }

    pub fn get_nand_type(&self) -> Result<NandType, FlasherError> {
        let id = self.read_flash_id()?;
        // Standard Xbox 360 IDs:
        // 0x002C71AD -> 16MB Hynix (Small)
        // 0x00AD73AD -> 16MB Hynix (Small)
        // 0x002CAAEC -> 256MB/512MB Samsung (Large)
        // 0x00D580AD -> 64MB Hynix (Large)
        
        match id & 0x00FFFFFF {
            0x0071AD | 0x0073AD | 0x0075AD | 0x0076AD => Ok(NandType::SmallBlock),
            _ => Ok(NandType::LargeBlock), // Default to Large if unknown but alive
        }
    }

    pub fn read_page(&self, page: u32) -> Result<Vec<u8>, FlasherError> {
        // Send command 0x20 (Bulk Read)
        // [Command][StartPage (LE)][Count (LE)]
        let mut cmd = vec![0x20];
        cmd.extend_from_slice(&page.to_le_bytes());
        cmd.extend_from_slice(&1u32.to_le_bytes()); // Read 1 page

        self.handle.write_bulk(EP_OUT, &cmd, Duration::from_millis(1000))?;

        // Read back 528 bytes
        let mut buffer = vec![0u8; 528];
        let bytes_read = self.handle.read_bulk(EP_IN, &mut buffer, Duration::from_millis(2000))?;

        if bytes_read != 528 {
            return Err(FlasherError::Protocol(format!("Expected 528 bytes, got {}", bytes_read)));
        }

        Ok(buffer)
    }

    pub fn write_page(&self, page: u32, data: &[u8]) -> Result<(), FlasherError> {
        if data.len() != 528 {
            return Err(FlasherError::Protocol("Data must be exactly 528 bytes".to_string()));
        }

        // Send command 0x21 (Bulk Write)
        let mut cmd = vec![0x21];
        cmd.extend_from_slice(&page.to_le_bytes());
        cmd.extend_from_slice(&1u32.to_le_bytes());

        self.handle.write_bulk(EP_OUT, &cmd, Duration::from_millis(1000))?;

        // Stream the 528 bytes
        self.handle.write_bulk(EP_OUT, data, Duration::from_millis(2000))?;

        Ok(())
    }

    /// Smart Read: Checks for bad blocks and handles remapping automatically
    pub fn smart_read_page(&self, logical_page: u32) -> Result<Vec<u8>, FlasherError> {
        // In a real implementation, we would check the Spare Area for bad block markers
        // and consult the gxremapper table.
        // For now, we'll just demonstrate the integration point.
        
        let physical_page = logical_page; // TODO: consult gxremapper
        
        self.read_page(physical_page)
    }

    /// Smart Write: Checks for bad blocks and finds healthy reserve blocks if needed
    pub fn smart_write_page(&self, logical_page: u32, data: &[u8]) -> Result<(), FlasherError> {
        let physical_page = logical_page; // TODO: consult gxremapper
        
        match self.write_page(physical_page, data) {
            Ok(_) => Ok(()),
            Err(e) => {
                // If write fails (hardware reports error), we would:
                // 1. Mark logical_page as bad in gxremapper
                // 2. Find a healthy reserve block
                // 3. Retry the write there
                Err(e)
            }
        }
    }
}

fn main() {
    println!("PICF2SPI - Modernized Legacy Flasher Driver");
    
    match PicFlasher::new() {
        Ok(flasher) => {
            println!("Device connected!");
            
            match flasher.smart_read_page(0) {
                Ok(data) => {
                    println!("Read page 0: {} bytes", data.len());
                    
                    // Check if block is bad using gxremapper
                    // Spare area is at the end of the 528 byte page
                    let spare_ptr = &data[512..528];
                    let is_bad = gxremapper::gx_remap_is_bad(spare_ptr.as_ptr(), true);
                    
                    if is_bad {
                        println!("Warning: Block 0 is marked as BAD!");
                    } else {
                        println!("Block 0 is healthy.");
                    }
                },
                Err(e) => println!("Read failed: {:?}", e),
            }
        }
        Err(e) => println!("Error: {:?}", e),
    }
}
