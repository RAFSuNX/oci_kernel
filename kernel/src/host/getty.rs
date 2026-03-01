/// Getty provides an interactive login prompt over a serial console.
///
/// In the bare-metal kernel, it reads from `serial::SERIAL` (uart_16550).
/// This module is hardware-dependent and is not compiled for host tests.

#[cfg(not(test))]
use alloc::{string::String, vec::Vec};

#[cfg(not(test))]
use super::shell::ShellCommand;

#[cfg(not(test))]
pub struct Getty;

#[cfg(not(test))]
impl Getty {
    pub fn new() -> Self { Self }

    /// Block indefinitely, presenting a login prompt then a shell.
    pub fn run(&mut self) -> ! {
        loop {
            self.println("OCI Kernel 0.1.0");
            self.print("login: ");
            let user = self.read_line();
            self.print("Password: ");
            let _pass = self.read_line();

            // Stub auth: any non-empty user is accepted.
            if !user.trim().is_empty() {
                self.run_shell();
            } else {
                self.println("Login incorrect.\n");
            }
        }
    }

    fn run_shell(&mut self) {
        self.println("Welcome to OCI Kernel. Type 'help' for available commands.");
        loop {
            self.print("$ ");
            let line = self.read_line();
            if line.trim().is_empty() {
                continue;
            }
            match ShellCommand::parse(&line) {
                Ok(cmd) => self.execute(cmd),
                Err(_)  => self.println("Unknown command. Type 'help' for available commands."),
            }
        }
    }

    fn execute(&mut self, cmd: ShellCommand) {
        match cmd {
            ShellCommand::Help => {
                self.println("Available commands:");
                self.println("  container run <image> [-p host:container] [-v src:dst[:ro]]");
                self.println("  container list | stop <id> | logs [-f] <id> | inspect <id>");
                self.println("  image pull <name:tag> | list | rm <name:tag>");
                self.println("  volume create <name> | rm <name>");
                self.println("  kernel info");
            }
            ShellCommand::KernelInfo => {
                self.println("OCI Kernel 0.1.0  arch=x86_64  build=no_std");
            }
            ShellCommand::ContainerList => {
                self.println("CONTAINER ID  IMAGE  STATUS");
                self.println("(no containers)");
            }
            ShellCommand::ImageList => {
                self.println("IMAGE  TAG  ID");
                self.println("(no images)");
            }
            _ => {
                self.println("Command not yet fully implemented.");
            }
        }
    }

    fn print(&mut self, s: &str) {
        use crate::serial_print;
        serial_print!("{}", s);
    }

    fn println(&mut self, s: &str) {
        use crate::serial_println;
        serial_println!("{}", s);
    }

    /// Read a line from the serial port (blocking, echoes characters).
    fn read_line(&mut self) -> String {
        use crate::serial_print;
        let mut buf = Vec::new();
        loop {
            let byte = read_serial_byte();
            match byte {
                b'\r' | b'\n' => {
                    serial_print!("\n");
                    break;
                }
                0x7f | 0x08 => {
                    // Backspace
                    if buf.pop().is_some() {
                        serial_print!("\x08 \x08");
                    }
                }
                c if c >= 0x20 => {
                    buf.push(c);
                    serial_print!("{}", c as char);
                }
                _ => {}
            }
        }
        String::from_utf8_lossy(&buf).into_owned()
    }
}

/// Block until a byte is available on COM1.
#[cfg(not(test))]
fn read_serial_byte() -> u8 {
    use x86_64::instructions::port::Port;
    let mut line_status: Port<u8> = Port::new(0x3FD); // COM1 line status register
    let mut data: Port<u8> = Port::new(0x3F8);        // COM1 data register
    loop {
        let status = unsafe { line_status.read() };
        if status & 0x01 != 0 {
            // Data ready
            return unsafe { data.read() };
        }
        core::hint::spin_loop();
    }
}
