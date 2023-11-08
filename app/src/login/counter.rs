use std::io;
use std::io::{stdout, Write};
use std::ops::ControlFlow;
use std::time::Duration;

use crossterm::{
    cursor, queue,
    style::Print,
    terminal::{self, ClearType},
};
use infrastructure::sync::timer;

const UNIT_SECOND: Duration = Duration::from_secs(1);

pub async fn counter(mut count: u64) -> io::Result<()> {
    timer::new_fn(UNIT_SECOND, || {
        if let e @ Err(_) = render(count) {
            return ControlFlow::Break(e);
        }

        if count == 0 {
            ControlFlow::Break(Ok(()))
        } else {
            count -= 1;
            ControlFlow::Continue(())
        }
    })
    .await
}

fn render(count: u64) -> io::Result<()> {
    queue!(
        stdout(),
        cursor::SavePosition,
        cursor::Hide,
        terminal::Clear(ClearType::CurrentLine),
        Print(format!("Time for verifying: {count}")),
        cursor::RestorePosition
    )?;
    io::stdout().flush()
}

pub fn recover_cursor() -> io::Result<()> {
    crossterm::execute!(stdout(), cursor::Show)
}
