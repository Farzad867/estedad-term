use std::num::NonZeroU32;
use std::rc::Rc;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};
use softbuffer::{Context, Surface};

struct App {
    window: Option<Rc<Window>>,
    context: Option<Context<Rc<Window>>>,
    surface: Option<Surface<Rc<Window>, Rc<Window>>>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let win = Rc::new(event_loop.create_window(Window::default_attributes().with_title("Estedad Term")).unwrap());
        let ctx = Context::new(win.clone()).unwrap();
        let surf = Surface::new(&ctx, win.clone()).unwrap();
        self.window = Some(win);
        self.context = Some(ctx);
        self.surface = Some(surf);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => {
                if let (Some(win), Some(surf)) = (&self.window, &mut self.surface) {
                    let size = win.inner_size();
                    if let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) {
                        surf.resize(w, h).unwrap();
                        let mut buf = surf.buffer_mut().unwrap();
                        buf.fill(0xFF181824); // Dark theme background
                        buf.present().unwrap();
                    }
                }
            }
            _ => (),
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().unwrap();
    let mut app = App { window: None, context: None, surface: None };
    let _ = event_loop.run_app(&mut app);
}
