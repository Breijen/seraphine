use x86_64::structures::paging::{FrameAllocator, Mapper, OffsetPageTable, Page, PageTableFlags, PhysFrame, Size4KiB};
use x86_64::{VirtAddr};

use crate::{serial_println};
use crate::hardware::pci::{read_pci_bar, get_pci_device};
use crate::hardware::pit::{timer_wait_ms};
use crate::mem::memory::map_nvme_base;

const NVME_RESET_TIMEOUT: u8 = 100;
const NVME_IDENTIFY_CNS: u32 = 1;
const QUEUE_SIZE: u32 = 256; // Maximum queue size
const ASQ_SIZE: usize = 64 * 64; // Admin Submission Queue size
const ACQ_SIZE: usize = 64 * 16;  // Admin Completion Queue size

const NVME_ADMIN_IDENTIFY: u8 = 0x06;
const NVME_ADMIN_SET_FEATURES: u8 = 0x09;
const NVME_FEAT_SOFTWARE_PROGRESS_MARKER: u8 = 0x80;

struct NvmeRegisters {
    nvme_base_addr: u64,
    nvme_virt_addr: VirtAddr,
    submission_queue_tail: u64,
    completion_queue_head: u64,
    doorbell_stride: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct NvmeCommand {
    opcode: u8,
    flags: u8,
    command_id: u16,
    namespace_id: u32,
    reserved1: u64,
    reserved2: u64,
    metadata_ptr: u64,
    prp1: u64,
    prp2: u64,
    command_specific: [u32; 6],
}

#[repr(C)]
#[derive(Debug)]
struct NvmeCompletion {
    command_specific: u32,
    reserved: u32,
    submission_queue_head: u16,
    submission_queue_id: u16,
    command_id: u16,
    phase_tag: u16,
    status: u16,
}

#[repr(C, packed)]
#[derive(Debug)] // Voeg Debug hier toe
struct NvmeIdentifyController {
    pci_vendor_id: u16,
    pci_subsystem_vendor_id: u16,
    serial_number: [u8; 20],
    model_number: [u8; 40],
    firmware_revision: [u8; 8],
    recommended_arbitration_burst: u8,
    ieee_oui_identifier: [u8; 3],
    controller_multi_path_io_and_namespace_sharing_capabilities: u8,
    maximum_data_transfer_size: u8,
}

impl NvmeRegisters {
    fn new(addr: u64) -> Self {
        let nvme_virt_addr = VirtAddr::new(0xffff_8000_0000_0000 + addr);

        NvmeRegisters {
            nvme_base_addr: addr,
            nvme_virt_addr,
            submission_queue_tail: 0,
            completion_queue_head: 0,
            doorbell_stride: 0,
        }
    }

    fn reset(&mut self) {
        // Read the CAP register
        let cap = self.nvme_read_reg64(0x00);

        // Extract fields from the CAP register
        let cqr = (cap & (1 << 0)) != 0;
        let mqes = (cap >> 1) & 0x7FFF; // Maximum Queue Entry Size
        let cms = (cap & (1 << 16)) != 0;
        let sqs = (cap >> 20) & 0x0F; // Submission Queue Size
        let aqs = (cap >> 24) & 0x0F; // Admin Queue Size
        let to = (cap >> 32) & 0xFF; // Timeout
        let dstrd = (cap >> 40) & 0x0F; // Doorbell stride

        // Print the fields
        serial_println!("NVMe Controller CAP Register Details:");
        serial_println!("CQR: {}", cqr);
        serial_println!("MQES: {}", mqes);
        serial_println!("CMS: {}", cms);
        serial_println!("SQS: {}", sqs);
        serial_println!("AQS: {}", aqs);
        serial_println!("TO: {}", to);
        serial_println!("DSTRD: {}", dstrd);

        self.doorbell_stride = dstrd as u32;

        // Reset the NVMe controller
        self.nvme_write_reg32(0x14, 0); // Reset command
        serial_println!("Sent reset command to NVMe");

        // Wait until the reset is complete
        self.wait_nvme_reset();
    }

    fn wait_nvme_reset(&self) {
        let mut timeout = NVME_RESET_TIMEOUT;

        while timeout > 0 {
            timer_wait_ms(1);  // Wait 1 ms
            let status = self.nvme_read_reg32(0x1C);

            if status == 0 {
                self.nvme_write_reg32(0x14, 1);  // Power on the controller
                return;
            }

            timeout -= 1;
        }

        serial_println!("NVMe reset timed out");
    }

    fn enable(&self) {
        timer_wait_ms(1);  // Wait for a moment

        let status = self.nvme_read_reg32(0x1C);
        serial_println!("STATUS: {:?}", status);

        if status == 1 {
            serial_println!("Successfully Reset NVMe");
        } else {
            serial_println!("NVMe enable failed with status: {:?}", status);
        }
    }

    fn send_init_command(&self) {
        let addr = self.nvme_virt_addr.as_u64();
        let current_value = unsafe { core::ptr::read_volatile(addr as *const u16) };

        // BIT 1, 2 and 10 should be enabled
        let values = current_value | (1 << 1) | (1 << 2) | (1 << 10);

        unsafe {
            core::ptr::write_volatile(addr as *mut u16, values);
        }
    }

    fn init_admin_queues(&mut self, mapper: &mut OffsetPageTable, frame_allocator: &mut impl FrameAllocator<Size4KiB>) {
        let asq_frame = self.allocate_frame(frame_allocator, "ASQ").expect("Failed to allocate ASQ frame");
        let acq_frame = self.allocate_frame(frame_allocator, "ACQ").expect("Failed to allocate ACQ frame");

        // Configure Admin Submission Queue and Admin Completion Queue
        self.configure_queues(asq_frame, acq_frame, mapper, frame_allocator);

        // Set queue sizes in the AQA register
        let queue_size = (QUEUE_SIZE - 1) | ((QUEUE_SIZE - 1) << 16);
        self.nvme_write_reg32(0x24, queue_size); // AQA register

        self.submission_queue_tail = 0;
        self.completion_queue_head = 0;

        serial_println!("NVMe Admin Queue initialized");
    }

    fn configure_queues(&mut self, asq_frame: PhysFrame<Size4KiB>, acq_frame: PhysFrame<Size4KiB>, mapper: &mut OffsetPageTable, frame_allocator: &mut impl FrameAllocator<Size4KiB>) {
        let asq_virt_addr = 0xffff800000000000 + asq_frame.start_address().as_u64();
        let acq_virt_addr = 0xffff800000000000 + acq_frame.start_address().as_u64();

        serial_println!("ASQ ADDRESS: {:X}", asq_virt_addr);
        serial_println!("ACQ ADDRESS: {:X}", acq_virt_addr);

        // ASQ en ACQ
        self.map_queue(mapper, asq_frame, asq_virt_addr, "ASQ", frame_allocator);
        self.map_queue(mapper, asq_frame, acq_virt_addr, "ACQ", frame_allocator);

        self.nvme_write_reg64(0x28, asq_virt_addr);  // ASQ
        self.nvme_write_reg64(0x30, acq_virt_addr);  // ACQ
    }

    fn map_queue(&self, mapper: &mut OffsetPageTable, frame: PhysFrame<Size4KiB>, addr: u64, name: &str, frame_allocator: &mut impl FrameAllocator<Size4KiB>) {
        let page: Page<Size4KiB> = Page::containing_address(VirtAddr::new(addr));
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;

        // Debug output
        serial_println!(
            "Mapping {}: Virt Addr: {:X} -> Phys Addr: {:X}",
            name,
            addr,
            frame.start_address().as_u64()
        );

        unsafe {
            if let Err(e) = mapper.map_to(page, frame, flags, frame_allocator) {
                serial_println!("Failed to map {}: {:?}", name, e);
                panic!("Mapping error");
            }
        }
    }

    fn allocate_frame(&self, frame_allocator: &mut impl FrameAllocator<Size4KiB>, name: &str) -> Result<PhysFrame<Size4KiB>, &'static str> {
        frame_allocator.allocate_frame().ok_or_else(|| {
            serial_println!("Failed to allocate frame for {}", name);
            "Allocation Error"
        })
    }

    fn send_identify_command(&mut self, cns: u8, nsid: u32, mapper: &mut OffsetPageTable, frame_allocator: &mut impl FrameAllocator<Size4KiB>) -> Result<(), &'static str> {
        let identify_data = self.allocate_frame(frame_allocator, "Identify Data")
            .expect("Failed to allocate Identify Data frame");

        serial_println!("Identify Data Frame Start Address: {:X}", identify_data.start_address().as_u64());

        let mut cmd = NvmeCommand {
            opcode: NVME_ADMIN_IDENTIFY,
            flags: 0,
            command_id: 0,
            namespace_id: nsid,
            reserved1: 0,
            reserved2: 0,
            metadata_ptr: 0,
            prp1: identify_data.start_address().as_u64(),
            prp2: 0,
            command_specific: [0; 6],
        };

        cmd.command_specific[0] = (cns as u32) & 0xFF;

        if cns == 0 {
            cmd.namespace_id = nsid; // Set the namespace ID
        }

        serial_println!("Submitting Identify command with CNS: {}", cns);

        self.submit_admin_command(cmd)?;

        // Read the Identify Data structure
        let identify_data_virt_addr = self.map_identify_data(identify_data, mapper, frame_allocator);
        let identify_data = unsafe { core::ptr::read_volatile(identify_data_virt_addr as *const NvmeIdentifyController) };

        // Check for IO capabilities
        if identify_data.controller_multi_path_io_and_namespace_sharing_capabilities != 0 {
            serial_println!("NVMe controller is an IO controller");
        } else {
            serial_println!("NVMe controller with address {:X?} is not an IO controller: {:?}", identify_data_virt_addr, identify_data);
        }

        Ok(())
    }

    fn is_controller_ready(&self) -> bool {
        let csts = self.nvme_read_reg32(0x1c);  // CSTS register
        (csts & 0x1) == 1  // Check RDY bit
    }

    fn map_identify_data(&self, identify_frame: PhysFrame<Size4KiB>, mapper: &mut OffsetPageTable, frame_allocator: &mut impl FrameAllocator<Size4KiB>) -> u64 {
        let identify_virt_addr = 0xffff800000000000 + identify_frame.start_address().as_u64();

        serial_println!(
            "Mapped Identify Frame: Physical Address: {:X}, Virtual Address: {:X}",
            identify_frame.start_address().as_u64(),
            identify_virt_addr
        );
        self.map_queue(mapper, identify_frame, identify_virt_addr, "Identify Data", frame_allocator);
        identify_virt_addr
    }

    fn submit_admin_command(&mut self, cmd: NvmeCommand) -> Result<(), &'static str> {
        // Submit the command to the Admin Submission Queue
        let asq_base_addr = self.nvme_read_reg64(0x28);
        let asq_addr = (asq_base_addr + (self.submission_queue_tail * core::mem::size_of::<NvmeCommand>() as u64)) as *mut NvmeCommand;

        serial_println!("Submission Queue Address: {:?}", asq_addr);

        unsafe {
            // Write the command to the submission queue
            core::ptr::write_volatile(asq_addr, cmd);
        }

        serial_println!("COMMAND: {:?}", cmd);
        serial_println!("PRP1 Physical Address: 0x{:X}", cmd.prp1);

        // Increment the Submission Queue Tail
        let old_tail = self.submission_queue_tail;
        self.submission_queue_tail = (self.submission_queue_tail + 1) % QUEUE_SIZE as u64;

        // Calculate the offset for the Submission Queue Tail Doorbell
        let sq_tail_doorbell_offset = 0x1000 + 2 * (4 << self.doorbell_stride);

        // Debug: Print values before writing
        serial_println!("Old Tail: {}, New Tail: {}", old_tail, self.submission_queue_tail);

        // Write to the Submission Queue Tail Doorbell Register
        self.nvme_write_reg32_no_address(asq_base_addr, sq_tail_doorbell_offset as u32, self.submission_queue_tail as u32);

        serial_println!("Command submitted successfully");

        // Wait for completion
        self.wait_for_completion()
    }

    fn wait_for_completion(&mut self) -> Result<(), &'static str> {
        let acq_addr = self.nvme_read_reg64(0x30) as *const NvmeCompletion;

        loop {
            // Read the completion entry
            let completion = unsafe { core::ptr::read_volatile(acq_addr.add(self.completion_queue_head as usize)) };

            serial_println!("Completion: {:?}", completion);

            // Check if the completion is valid
            if (completion.phase_tag & 1) == (self.completion_queue_head & 1) as u16 {
                // Process the completion
                self.completion_queue_head = (self.completion_queue_head + 1) % QUEUE_SIZE as u64;
                self.nvme_write_reg32_no_address(self.nvme_read_reg64(0x28),0x1000 + 3 * (4 << self.doorbell_stride as u64), self.completion_queue_head as u32);

                // Check status of the command
                if self.completion_queue_head == QUEUE_SIZE as u64 {
                    self.completion_queue_head = 0;
                }

                let status = completion.status;
                serial_println!("Completion Status: 0x{:X}", status);
                if (status >> 1) != 0 {
                    let status_code_type = (status >> 9) & 0x7;
                    let status_code = (status >> 1) & 0xFF;
                    serial_println!("Command failed. Status Code Type: {}, Status Code: {}", status_code_type, status_code);
                    return Err("Command failed");
                }

                // Process the Identify data here (if applicable)

                return Ok(());
            }

            // Add a small delay or yield here to prevent tight looping
            timer_wait_ms(50);
        }
    }

    // Read & Write NVMe registers
    fn nvme_read_reg32(&self, offset: u32) -> u32 {
        unsafe {
            let nvme_reg = (self.nvme_virt_addr.as_u64() + offset as u64) as *const u32;
            core::ptr::read_volatile(nvme_reg)
        }
    }

    fn nvme_read_reg64(&self, offset: u32) -> u64 {
        unsafe {
            let nvme_reg = (self.nvme_virt_addr.as_u64() + offset as u64) as *const u64;
            core::ptr::read_volatile(nvme_reg)
        }
    }

    fn nvme_write_reg32(&self, offset: u32, value: u32) {
        unsafe {
            let reg_addr = (self.nvme_virt_addr.as_u64() + offset as u64) as *mut u32;
            core::ptr::write_volatile(reg_addr, value);
        }
    }

    fn nvme_write_reg32_no_address(&self, addr: u64, offset: u32, value: u32) {
        unsafe {
            let reg_addr = (addr + offset as u64) as *mut u32;
            core::ptr::write_volatile(reg_addr, value);
        }
    }

    fn nvme_write_reg64(&self, offset: u32, value: u64) {
        unsafe {
            let reg_addr = (self.nvme_virt_addr.as_u64() + offset as u64) as *mut u64;
            core::ptr::write_volatile(reg_addr, value);
        }
    }
}

static mut CONTROLLER: Option<NvmeRegisters> = None;

pub fn init_controller(mapper: &mut OffsetPageTable, frame_allocator: &mut impl FrameAllocator<Size4KiB>) {
    let nvme_base_addr = find_first_nvme();

    unsafe {
        CONTROLLER = Some(NvmeRegisters::new(nvme_base_addr));

        if let Some(controller) = CONTROLLER.as_mut() {
            map_nvme_base(controller.nvme_base_addr, controller.nvme_virt_addr, mapper, frame_allocator);

            // Enable Interrupts, bus-mastering and memory space access
            controller.send_init_command();

            // Initialize NVMe controller
            controller.reset();
            controller.init_admin_queues(mapper, frame_allocator);
            controller.enable();

            if controller.is_controller_ready() {
                controller.send_identify_command(NVME_IDENTIFY_CNS as u8, 0, mapper, frame_allocator)
                    .expect("Failed to send Identify Controller command");
            }


        } else {
            panic!("Failed to initialize NVMe controller");
        }
    }
}

pub fn find_first_nvme() -> u64 {
    for bus in 0..=255 {
        for device in 0..31 {
            for function in 0..7 {
                if let Some(pci_device) = get_pci_device(bus, device, function) {
                    if pci_device.class_code == 0x01 && pci_device.subclass_code == 0x08 {
                        return get_nvme_base_addr(bus, device, function);
                    }
                }
            }
        }
    }

    0
}

fn get_nvme_base_addr(bus: u8, device: u8, function: u8) -> u64 {
    let bar0 = read_pci_bar(bus, device, function, 0); // BAR0
    let bar1 = read_pci_bar(bus, device, function, 1); // BAR1

    // Combine BAR0 and BAR1 into a 64-bit MMIO address
    ((bar1 as u64) << 32) | (bar0 as u64 & 0xFFFFFFF0)
}