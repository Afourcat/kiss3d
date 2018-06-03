//! The kiss3d window.
/*
 * FIXME: this file is too big. Some heavy refactoring need to be done here.
 */
use std::cell::RefCell;
use std::iter::repeat;
use std::mem;
use std::path::Path;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Once, ONCE_INIT};
use std::thread;
use std::time::{Duration, Instant};

use na::{Point2, Point3, Vector2, Vector3};

use camera::{ArcBall, Camera};
use context::Context;
use image::imageops;
use image::{ImageBuffer, Rgb};
use light::Light;
use line_renderer::LineRenderer;
use ncollide3d::procedural::TriMesh;
use point_renderer::PointRenderer;
use post_processing::PostProcessingEffect;
use resource::{FramebufferManager, Mesh, RenderTarget, Texture, TextureManager};
use scene::SceneNode;
// use text::{Font, TextRenderer};
use event::{Action, EventManager, Key, WindowEvent};
use window::{Canvas, State};

static DEFAULT_WIDTH: u32 = 800u32;
static DEFAULT_HEIGHT: u32 = 600u32;

/// Structure representing a window and a 3D scene.
///
/// This is the main interface with the 3d engine.
pub struct Window {
    events: Rc<Receiver<WindowEvent>>,
    unhandled_events: Rc<RefCell<Vec<WindowEvent>>>,
    canvas: Canvas,
    max_dur_per_frame: Option<Duration>,
    scene: SceneNode,
    light_mode: Light, // FIXME: move that to the scene graph
    background: Vector3<f32>,
    line_renderer: LineRenderer,
    point_renderer: PointRenderer,
    // text_renderer: TextRenderer,
    framebuffer_manager: FramebufferManager,
    post_process_render_target: RenderTarget,
    curr_time: usize, // Instant,
    camera: Rc<RefCell<ArcBall>>,
}

impl Window {
    /// Indicates whether this window should be closed.
    #[inline]
    pub fn should_close(&self) -> bool {
        self.canvas.should_close()
    }

    /// The window width.
    #[inline]
    pub fn width(&self) -> u32 {
        self.canvas.size().0
    }

    /// The window height.
    #[inline]
    pub fn height(&self) -> u32 {
        self.canvas.size().1
    }

    /// The size of the window.
    #[inline]
    pub fn size(&self) -> Vector2<u32> {
        let (w, h) = self.canvas.size();
        Vector2::new(w, h)
    }

    /// Sets the maximum number of frames per second. Cannot be 0. `None` means there is no limit.
    #[inline]
    pub fn set_framerate_limit(&mut self, fps: Option<u64>) {
        self.max_dur_per_frame = fps.map(|f| {
            assert!(f != 0);
            Duration::from_millis(1000 / f)
        })
    }

    /// Set window title
    pub fn set_title(&mut self, title: &str) {
        self.canvas.set_title(title)
    }

    /// Closes the window.
    #[inline]
    pub fn close(&mut self) {
        self.canvas.close()
    }

    /// Hides the window, without closing it. Use `show` to make it visible again.
    #[inline]
    pub fn hide(&mut self) {
        self.canvas.hide()
    }

    /// Makes the window visible. Use `hide` to hide it.
    #[inline]
    pub fn show(&mut self) {
        self.canvas.show()
    }

    /// Sets the background color.
    #[inline]
    pub fn set_background_color(&mut self, r: f32, g: f32, b: f32) {
        self.background.x = r;
        self.background.y = g;
        self.background.z = b;
    }

    /// Set the size of all subsequent points to be drawn until the next time this function is envoked.
    #[inline]
    pub fn set_point_size(&mut self, pt_size: f32) {
        self.point_renderer.set_point_size(pt_size);
    }

    // XXX: remove this (moved to the render_frame).
    /// Adds a line to be drawn during the next frame.
    #[inline]
    pub fn draw_line(&mut self, a: &Point3<f32>, b: &Point3<f32>, color: &Point3<f32>) {
        self.line_renderer
            .draw_line(a.clone(), b.clone(), color.clone());
    }

    // XXX: remove this (moved to the render_frame).
    /// Adds a point to be drawn during the next frame.
    #[inline]
    pub fn draw_point(&mut self, pt: &Point3<f32>, color: &Point3<f32>) {
        self.point_renderer.draw_point(pt.clone(), color.clone());
    }

    // // XXX: remove this (moved to the render_frame).
    // /// Adds a string to be drawn during the next frame.
    // #[inline]
    // pub fn draw_text(
    //     &mut self,
    //     text: &str,
    //     pos: &Point2<f32>,
    //     font: &Rc<Font>,
    //     color: &Point3<f32>,
    // ) {
    //     self.text_renderer.draw_text(text, pos, font, color);
    // }

    /// Removes an object from the scene.
    pub fn remove(&mut self, sn: &mut SceneNode) {
        sn.unlink()
    }

    /// Adds a group to the scene.
    ///
    /// A group is a node not containing any object.
    pub fn add_group(&mut self) -> SceneNode {
        self.scene.add_group()
    }

    /// Adds an obj model to the scene.
    ///
    /// # Arguments
    /// * `path`  - relative path to the obj file.
    /// * `scale` - scale to apply to the model.
    pub fn add_obj(&mut self, path: &Path, mtl_dir: &Path, scale: Vector3<f32>) -> SceneNode {
        self.scene.add_obj(path, mtl_dir, scale)
    }

    /// Adds an unnamed mesh to the scene.
    pub fn add_mesh(&mut self, mesh: Rc<RefCell<Mesh>>, scale: Vector3<f32>) -> SceneNode {
        self.scene.add_mesh(mesh, scale)
    }

    /// Creates and adds a new object using the geometry generated by a given procedural generator.
    /// Creates and adds a new object using a mesh descriptor.
    pub fn add_trimesh(&mut self, descr: TriMesh<f32>, scale: Vector3<f32>) -> SceneNode {
        self.scene.add_trimesh(descr, scale)
    }

    /// Creates and adds a new object using the geometry registered as `geometry_name`.
    pub fn add_geom_with_name(
        &mut self,
        geometry_name: &str,
        scale: Vector3<f32>,
    ) -> Option<SceneNode> {
        self.scene.add_geom_with_name(geometry_name, scale)
    }

    /// Adds a cube to the scene. The cube is initially axis-aligned and centered at (0, 0, 0).
    ///
    /// # Arguments
    /// * `wx` - the cube extent along the z axis
    /// * `wy` - the cube extent along the y axis
    /// * `wz` - the cube extent along the z axis
    pub fn add_cube(&mut self, wx: f32, wy: f32, wz: f32) -> SceneNode {
        self.scene.add_cube(wx, wy, wz)
    }

    /// Adds a sphere to the scene. The sphere is initially centered at (0, 0, 0).
    ///
    /// # Arguments
    /// * `r` - the sphere radius
    pub fn add_sphere(&mut self, r: f32) -> SceneNode {
        self.scene.add_sphere(r)
    }

    /// Adds a cone to the scene. The cone is initially centered at (0, 0, 0) and points toward the
    /// positive `y` axis.
    ///
    /// # Arguments
    /// * `h` - the cone height
    /// * `r` - the cone base radius
    pub fn add_cone(&mut self, r: f32, h: f32) -> SceneNode {
        self.scene.add_cone(r, h)
    }

    /// Adds a cylinder to the scene. The cylinder is initially centered at (0, 0, 0) and has its
    /// principal axis aligned with the `y` axis.
    ///
    /// # Arguments
    /// * `h` - the cylinder height
    /// * `r` - the cylinder base radius
    pub fn add_cylinder(&mut self, r: f32, h: f32) -> SceneNode {
        self.scene.add_cylinder(r, h)
    }

    /// Adds a capsule to the scene. The capsule is initially centered at (0, 0, 0) and has its
    /// principal axis aligned with the `y` axis.
    ///
    /// # Arguments
    /// * `h` - the capsule height
    /// * `r` - the capsule caps radius
    pub fn add_capsule(&mut self, r: f32, h: f32) -> SceneNode {
        self.scene.add_capsule(r, h)
    }

    /// Adds a double-sided quad to the scene. The quad is initially centered at (0, 0, 0). The
    /// quad itself is composed of a user-defined number of triangles regularly spaced on a grid.
    /// This is the main way to draw height maps.
    ///
    /// # Arguments
    /// * `w` - the quad width.
    /// * `h` - the quad height.
    /// * `wsubdivs` - number of horizontal subdivisions. This correspond to the number of squares
    /// which will be placed horizontally on each line. Must not be `0`.
    /// * `hsubdivs` - number of vertical subdivisions. This correspond to the number of squares
    /// which will be placed vertically on each line. Must not be `0`.
    /// update.
    pub fn add_quad(&mut self, w: f32, h: f32, usubdivs: usize, vsubdivs: usize) -> SceneNode {
        self.scene.add_quad(w, h, usubdivs, vsubdivs)
    }

    /// Adds a double-sided quad with the specified vertices.
    pub fn add_quad_with_vertices(
        &mut self,
        vertices: &[Point3<f32>],
        nhpoints: usize,
        nvpoints: usize,
    ) -> SceneNode {
        self.scene
            .add_quad_with_vertices(vertices, nhpoints, nvpoints)
    }

    #[doc(hidden)]
    pub fn add_texture(&mut self, path: &Path, name: &str) -> Rc<Texture> {
        TextureManager::get_global_manager(|tm| tm.add(path, name))
    }

    /// Returns whether this window is closed or not.
    pub fn is_closed(&self) -> bool {
        false // FIXME
    }

    /// Sets the light mode. Only one light is supported.
    pub fn set_light(&mut self, pos: Light) {
        self.light_mode = pos;
    }

    /// Opens a window, hide it then calls a user-defined procedure.
    ///
    /// # Arguments
    /// * `title` - the window title
    pub fn new_hidden(title: &str) -> Window {
        Window::do_new(title, true, DEFAULT_WIDTH, DEFAULT_HEIGHT)
    }

    /// Opens a window then calls a user-defined procedure.
    ///
    /// # Arguments
    /// * `title` - the window title
    pub fn new(title: &str) -> Window {
        Window::do_new(title, false, DEFAULT_WIDTH, DEFAULT_HEIGHT)
    }

    /// Opens a window with a custom size then calls a user-defined procedure.
    ///
    /// # Arguments
    /// * `title` - the window title.
    /// * `width` - the window width.
    /// * `height` - the window height.
    pub fn new_with_size(title: &str, width: u32, height: u32) -> Window {
        Window::do_new(title, false, width, height)
    }

    // FIXME: make this pub?
    fn do_new(title: &str, hide: bool, width: u32, height: u32) -> Window {
        let (event_send, event_receive) = mpsc::channel();
        let canvas = Canvas::open(title, hide, width, height, event_send);

        init_gl();

        let mut usr_window = Window {
            max_dur_per_frame: None,
            canvas: canvas,
            events: Rc::new(event_receive),
            unhandled_events: Rc::new(RefCell::new(Vec::new())),
            scene: SceneNode::new_empty(),
            light_mode: Light::Absolute(Point3::new(0.0, 10.0, 0.0)),
            background: Vector3::new(0.0, 0.0, 0.0),
            line_renderer: LineRenderer::new(),
            point_renderer: PointRenderer::new(),
            // text_renderer: TextRenderer::new(),
            post_process_render_target: FramebufferManager::new_render_target(
                width as usize,
                height as usize,
            ),
            framebuffer_manager: FramebufferManager::new(),
            curr_time: 0, // Instant::now(),
            camera: Rc::new(RefCell::new(ArcBall::new(
                Point3::new(0.0f32, 0.0, -1.0),
                Point3::origin(),
            ))),
        };

        if hide {
            usr_window.canvas.hide()
        }

        // usr_window.framebuffer_size_callback(DEFAULT_WIDTH, DEFAULT_HEIGHT);
        let light = usr_window.light_mode.clone();
        usr_window.set_light(light);

        usr_window
    }

    /// Reference to the scene associated with this window.
    #[inline]
    pub fn scene(&self) -> &SceneNode {
        &self.scene
    }

    /// Mutable reference to the scene associated with this window.
    #[inline]
    pub fn scene_mut(&mut self) -> &mut SceneNode {
        &mut self.scene
    }

    // FIXME: give more options for the snap size and offset.
    /// Read the pixels currently displayed to the screen.
    ///
    /// # Arguments:
    /// * `out` - the output buffer. It is automatically resized.
    pub fn snap(&self, out: &mut Vec<u8>) {
        let (width, height) = self.canvas.size();
        self.snap_rect(out, 0, 0, width as usize, height as usize)
    }

    /// Read a section of pixels from the screen
    ///
    /// # Arguments:
    /// * `out` - the output buffer. It is automatically resized
    /// * `x, y, width, height` - the rectangle to capture
    pub fn snap_rect(&self, out: &mut Vec<u8>, x: usize, y: usize, width: usize, height: usize) {
        let size = (width * height * 3) as usize;

        if out.len() < size {
            let diff = size - out.len();
            out.extend(repeat(0).take(diff));
        } else {
            out.truncate(size)
        }

        // FIXME: this is _not_ the fastest way of doing this.
        let ctxt = Context::get();
        ctxt.pixel_storei(Context::PACK_ALIGNMENT, 1);
        ctxt.read_pixels(
            x as i32,
            y as i32,
            width as i32,
            height as i32,
            Context::RGB,
            Some(out),
        );
    }

    /// Get the current screen as an image
    pub fn snap_image(&self) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
        let (width, height) = self.canvas.size();
        let mut buf = Vec::new();
        self.snap(&mut buf);
        let img_opt = ImageBuffer::from_vec(width as u32, height as u32, buf);
        let img = img_opt.expect("Buffer created from window was not big enough for image.");
        imageops::flip_vertical(&img)
    }

    /// Gets the events manager that gives access to an event iterator.
    pub fn events(&self) -> EventManager {
        EventManager::new(self.events.clone(), self.unhandled_events.clone())
    }

    /// Retrueve the status of a key.
    pub fn get_key(&self, key: Key) -> Action {
        self.canvas.get_key(key)
    }

    /// Poll events and pass them to a user-defined function. If the function returns `true`, the
    /// default engine event handler (camera, framebuffer size, etc.) is executed, if it returns
    /// `false`, the default engine event handler is not executed. Return `false` if you want to
    /// override the default engine behaviour.
    #[inline]
    fn handle_events(&mut self, camera: &mut Option<&mut Camera>) {
        let unhandled_events = self.unhandled_events.clone(); // FIXME: this is very ugly.
        let events = self.events.clone(); // FIXME: this is very ugly

        for event in unhandled_events.borrow().iter() {
            self.handle_event(camera, event)
        }

        for event in events.try_iter() {
            self.handle_event(camera, &event)
        }

        unhandled_events.borrow_mut().clear();
        self.canvas.poll_events();
    }

    fn handle_event(&mut self, camera: &mut Option<&mut Camera>, event: &WindowEvent) {
        match *event {
            WindowEvent::Key(Key::Escape, _, Action::Release, _) => {
                self.close();
            }
            WindowEvent::FramebufferSize(w, h) => {
                self.update_viewport(w as f32, h as f32);
            }
            _ => {}
        }

        match *camera {
            Some(ref mut cam) => cam.handle_event(&self.canvas, event),
            None => self.camera.borrow_mut().handle_event(&self.canvas, event),
        }
    }

    pub fn render_loop<S: State>(mut self, mut state: S) {
        Canvas::render_loop(move |_| self.render_with_state(&mut state))
    }

    pub fn render_with_state<S: State>(&mut self, state: &mut S) {
        {
            let (camera, effect) = state.camera_and_effect();
            self.render_with(camera, effect);
        }
        state.step(self)
    }

    /// Renders the scene using the default camera.
    ///
    /// Returns `false` if the window should be closed.
    pub fn render(&mut self) -> bool {
        self.render_with(None, None)
    }

    /// Render using a specific post processing effect.
    ///
    /// Returns `false` if the window should be closed.
    pub fn render_with_effect(&mut self, effect: &mut (PostProcessingEffect)) -> bool {
        self.render_with(None, Some(effect))
    }

    /// Render using a specific camera.
    ///
    /// Returns `false` if the window should be closed.
    pub fn render_with_camera(&mut self, camera: &mut (Camera)) -> bool {
        self.render_with(Some(camera), None)
    }

    /// Render using a specific camera and post processing effect.
    ///
    /// Returns `false` if the window should be closed.
    pub fn render_with_camera_and_effect(
        &mut self,
        camera: &mut Camera,
        effect: &mut PostProcessingEffect,
    ) -> bool {
        self.render_with(Some(camera), Some(effect))
    }

    /// Draws the scene with the given camera and post-processing effect.
    ///
    /// Returns `false` if the window should be closed.
    pub fn render_with(
        &mut self,
        camera: Option<&mut Camera>,
        post_processing: Option<&mut PostProcessingEffect>,
    ) -> bool {
        let mut camera = camera;
        self.handle_events(&mut camera);

        match camera {
            Some(cam) => self.do_render_with(cam, post_processing),
            None => {
                let self_cam = self.camera.clone(); // FIXME: this is ugly.
                let mut bself_cam = self_cam.borrow_mut();
                self.do_render_with(&mut *bself_cam, post_processing)
            }
        }
    }

    fn do_render_with(
        &mut self,
        camera: &mut Camera,
        post_processing: Option<&mut PostProcessingEffect>,
    ) -> bool {
        // XXX: too bad we have to do this at each frame…
        let w = self.width();
        let h = self.height();

        camera.handle_event(&self.canvas, &WindowEvent::FramebufferSize(w, h));
        camera.update(&self.canvas);

        match self.light_mode {
            Light::StickToCamera => self.set_light(Light::StickToCamera),
            _ => {}
        }

        {
            let mut post_processing = post_processing;
            if post_processing.is_some() {
                // if we need post-processing, render to our own frame buffer
                self.framebuffer_manager
                    .select(&self.post_process_render_target);
            } else {
                self.framebuffer_manager
                    .select(&FramebufferManager::screen());
            }

            for pass in 0usize..camera.num_passes() {
                camera.start_pass(pass, &self.canvas);
                self.render_scene(camera, pass);
            }
            camera.render_complete(&self.canvas);

            let (znear, zfar) = camera.clip_planes();

            // FIXME: remove this completely?
            // swatch off the wireframe mode for post processing and text rendering.
            // if self.wireframe_mode {
            //     verify!(gl::PolygonMode(Context::FRONT_AND_BACK, Context::FILL));
            // }

            match post_processing {
                Some(ref mut p) => {
                    // switch back to the screen framebuffer …
                    self.framebuffer_manager
                        .select(&FramebufferManager::screen());
                    // … and execute the post-process
                    // FIXME: use the real time value instead of 0.016!
                    p.update(0.016, w as f32, h as f32, znear, zfar);
                    p.draw(&self.post_process_render_target);
                }
                None => {}
            }

            // self.text_renderer.render(w, h);

            // We are done: swap buffers
            self.canvas.swap_buffers();

            // Limit the fps if needed.
            /*
            match self.max_dur_per_frame {
                None => {}
                Some(dur) => {
                    let elapsed = self.curr_time.elapsed();
                    if elapsed < dur {
                        thread::sleep(dur - elapsed);
                    }
                }
            }*/

            self.curr_time = 0; // Instant::now();

            // self.transparent_objects.clear();
            // self.opaque_objects.clear();
        }

        !self.should_close()
    }

    fn render_scene(&mut self, camera: &mut Camera, pass: usize) {
        let ctxt = Context::get();
        // Activate the default texture
        verify!(ctxt.active_texture(Context::TEXTURE0));
        // Clear the screen to black
        verify!(ctxt.clear_color(self.background.x, self.background.y, self.background.z, 1.0));
        verify!(ctxt.clear(Context::COLOR_BUFFER_BIT));
        verify!(ctxt.clear(Context::DEPTH_BUFFER_BIT));

        if self.line_renderer.needs_rendering() {
            self.line_renderer.render(pass, camera);
        }

        if self.point_renderer.needs_rendering() {
            self.point_renderer.render(pass, camera);
        }

        self.scene.data_mut().render(pass, camera, &self.light_mode);
    }

    fn update_viewport(&mut self, w: f32, h: f32) {
        // Update the viewport
        verify!(Context::get().scissor(0, 0, w as i32, h as i32));
        FramebufferManager::screen().resize(w, h);
        self.post_process_render_target.resize(w, h);
    }
}

fn init_gl() {
    /*
     * Misc configurations
     */
    let ctxt = Context::get();
    verify!(ctxt.front_face(Context::CCW));
    verify!(ctxt.enable(Context::DEPTH_TEST));
    verify!(ctxt.enable(Context::SCISSOR_TEST));
    #[cfg(not(any(target_arch = "wasm32", target_arch = "asmjs")))]
    {
        verify!(ctxt.enable(Context::PROGRAM_POINT_SIZE));
    }
    verify!(ctxt.depth_func(Context::LEQUAL));
    verify!(ctxt.front_face(Context::CCW));
    verify!(ctxt.enable(Context::CULL_FACE));
    verify!(ctxt.cull_face(Context::BACK));
}
