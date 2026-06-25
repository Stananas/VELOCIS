use std::cell::RefCell;
use wasm_bindgen::prelude::*;

thread_local! {
    static RENDERER: RefCell<Option<Renderer>> = const { RefCell::new(None) };
    static TEX_RENDERER: RefCell<Option<TexRenderer>> = const { RefCell::new(None) };
}

struct Renderer {
    prog: web_sys::WebGlProgram,
    buf: web_sys::WebGlBuffer,
    a_pos: u32,
}

struct TexRenderer {
    prog: web_sys::WebGlProgram,
    buf: web_sys::WebGlBuffer,
    a_pos: u32,
    a_uv: u32,
}

fn compile_shader(
    gl: &web_sys::WebGl2RenderingContext,
    kind: u32,
    source: &str,
) -> web_sys::WebGlShader {
    let shader = gl.create_shader(kind).unwrap();
    gl.shader_source(&shader, source);
    gl.compile_shader(&shader);
    shader
}

#[wasm_bindgen]
pub fn init_renderer(gl: &web_sys::WebGl2RenderingContext) {
    use web_sys::WebGl2RenderingContext as GL;

    let vs_src = "\
        #version 300 es\n\
        in vec2 a_pos;\n\
        void main() {\
            gl_Position = vec4(a_pos, 0.0, 1.0);\n\
        }";

    let fs_src = "\
        #version 300 es\n\
        precision highp float;\n\
        uniform vec4 u_color;\n\
        out vec4 frag_color;\n\
        void main() {\
            frag_color = u_color;\n\
        }";

    let vs = compile_shader(gl, GL::VERTEX_SHADER, vs_src);
    let fs = compile_shader(gl, GL::FRAGMENT_SHADER, fs_src);

    let prog = gl.create_program().unwrap();
    gl.attach_shader(&prog, &vs);
    gl.attach_shader(&prog, &fs);
    gl.link_program(&prog);

    let verts: [f32; 8] = [-0.5, -0.5, 0.5, -0.5, -0.5, 0.5, 0.5, 0.5];
    let verts_array = js_sys::Float32Array::from(&verts[..]);

    let buf = gl.create_buffer().unwrap();
    gl.bind_buffer(GL::ARRAY_BUFFER, Some(&buf));
    gl.buffer_data_with_array_buffer_view(GL::ARRAY_BUFFER, &verts_array, GL::STATIC_DRAW);

    let a_pos = gl.get_attrib_location(&prog, "a_pos") as u32;

    RENDERER.with(|r| {
        *r.borrow_mut() = Some(Renderer { prog, buf, a_pos });
    });
}

#[wasm_bindgen]
pub fn render_frame(gl: &web_sys::WebGl2RenderingContext, width: u32, height: u32, time: f64) {
    use web_sys::WebGl2RenderingContext as GL;

    gl.viewport(0, 0, width as i32, height as i32);
    gl.clear_color(0.02, 0.03, 0.04, 1.0);
    gl.clear(GL::COLOR_BUFFER_BIT);

    RENDERER.with(|r| {
        let renderer = r.borrow();
        let renderer = renderer.as_ref().unwrap();

        gl.use_program(Some(&renderer.prog));
        gl.bind_buffer(GL::ARRAY_BUFFER, Some(&renderer.buf));
        gl.enable_vertex_attrib_array(renderer.a_pos);
        gl.vertex_attrib_pointer_with_i32(renderer.a_pos, 2, GL::FLOAT, false, 0, 0);

        let t = time / 1000.0;
        let x_offset = (t * 0.8).sin() * 0.3;
        let y_offset = (t * 0.6).cos() * 0.2;
        let r = ((t * 0.5).sin() * 0.5 + 0.5) as f32;
        let g = ((t * 0.7 + 2.0).sin() * 0.5 + 0.5) as f32;
        let b = ((t * 0.3 + 4.0).sin() * 0.5 + 0.5) as f32;

        let u_color = gl.get_uniform_location(&renderer.prog, "u_color");
        gl.uniform4f(u_color.as_ref(), r, g, b, 1.0f32);

        let model = js_sys::Float32Array::from(
            &[
                -0.4f32 + x_offset as f32, -0.4f32 + y_offset as f32,
                 0.4f32 + x_offset as f32, -0.4f32 + y_offset as f32,
                -0.4f32 + x_offset as f32,  0.4f32 + y_offset as f32,
                 0.4f32 + x_offset as f32,  0.4f32 + y_offset as f32,
            ][..],
        );
        gl.buffer_data_with_array_buffer_view(GL::ARRAY_BUFFER, &model, GL::DYNAMIC_DRAW);

        gl.draw_arrays(GL::TRIANGLE_STRIP, 0, 4);
    });
}

#[wasm_bindgen]
pub fn init_texture_renderer(gl: &web_sys::WebGl2RenderingContext) {
    use web_sys::WebGl2RenderingContext as GL;

    let vs_src = "\
        #version 300 es\n\
        in vec2 a_pos;\n\
        in vec2 a_uv;\n\
        out vec2 v_uv;\n\
        void main() {\
            gl_Position = vec4(a_pos, 0.0, 1.0);\n\
            v_uv = a_uv;\n\
        }";

    let fs_src = "\
        #version 300 es\n\
        precision highp float;\n\
        in vec2 v_uv;\n\
        uniform sampler2D u_tex;\n\
        out vec4 frag_color;\n\
        void main() {\
            frag_color = texture(u_tex, v_uv);\n\
        }";

    let vs = compile_shader(gl, GL::VERTEX_SHADER, vs_src);
    let fs = compile_shader(gl, GL::FRAGMENT_SHADER, fs_src);

    let prog = gl.create_program().unwrap();
    gl.attach_shader(&prog, &vs);
    gl.attach_shader(&prog, &fs);
    gl.link_program(&prog);

    let verts: [f32; 16] = [
        -1.0, -1.0,  0.0, 1.0,
         1.0, -1.0,  1.0, 1.0,
        -1.0,  1.0,  0.0, 0.0,
         1.0,  1.0,  1.0, 0.0,
    ];
    let verts_array = js_sys::Float32Array::from(&verts[..]);

    let buf = gl.create_buffer().unwrap();
    gl.bind_buffer(GL::ARRAY_BUFFER, Some(&buf));
    gl.buffer_data_with_array_buffer_view(GL::ARRAY_BUFFER, &verts_array, GL::STATIC_DRAW);

    let a_pos = gl.get_attrib_location(&prog, "a_pos") as u32;
    let a_uv = gl.get_attrib_location(&prog, "a_uv") as u32;

    TEX_RENDERER.with(|r| {
        *r.borrow_mut() = Some(TexRenderer { prog, buf, a_pos, a_uv });
    });
}

#[wasm_bindgen]
pub fn render_texture_frame(gl: &web_sys::WebGl2RenderingContext, width: u32, height: u32) {
    use web_sys::WebGl2RenderingContext as GL;

    gl.viewport(0, 0, width as i32, height as i32);
    gl.clear_color(0.02, 0.03, 0.04, 1.0);
    gl.clear(GL::COLOR_BUFFER_BIT);
    gl.disable(GL::CULL_FACE);
    gl.disable(GL::DEPTH_TEST);

    TEX_RENDERER.with(|r| {
        let renderer = r.borrow();
        let renderer = renderer.as_ref().unwrap();

        gl.use_program(Some(&renderer.prog));
        gl.bind_buffer(GL::ARRAY_BUFFER, Some(&renderer.buf));

        let stride = 16;
        gl.enable_vertex_attrib_array(renderer.a_pos);
        gl.vertex_attrib_pointer_with_i32(renderer.a_pos, 2, GL::FLOAT, false, stride, 0);
        gl.enable_vertex_attrib_array(renderer.a_uv);
        gl.vertex_attrib_pointer_with_i32(renderer.a_uv, 2, GL::FLOAT, false, stride, 8);

        gl.uniform1i(gl.get_uniform_location(&renderer.prog, "u_tex").as_ref(), 0);

        gl.draw_arrays(GL::TRIANGLE_STRIP, 0, 4);
    });
}
