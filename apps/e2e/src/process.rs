use std::process::{Child, Command, Stdio};

pub struct KillOnDrop(Option<Child>);

impl KillOnDrop {
    pub fn new(child: Child) -> Self {
        Self(Some(child))
    }

    pub fn kill_now(&mut self) {
        if let Some(mut c) = self.0.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        self.kill_now();
    }
}

pub fn null_stdio(cmd: &mut Command) -> &mut Command {
    cmd.stdin(Stdio::null());
    cmd
}
