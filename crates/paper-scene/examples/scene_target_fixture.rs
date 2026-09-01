use std::path::PathBuf;

fn push_len_string(output: &mut Vec<u8>, value: &str) {
    output.extend_from_slice(&(value.len() as u32).to_le_bytes());
    output.extend_from_slice(value.as_bytes());
}

fn push_nul_string(output: &mut Vec<u8>, value: &str) {
    output.extend_from_slice(value.as_bytes());
    output.push(0);
}

fn push_i32(output: &mut Vec<u8>, value: i32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn package(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut output = Vec::new();
    push_len_string(&mut output, "PKGV0007");
    push_i32(&mut output, files.len() as i32);
    let mut offset = 0u32;
    for (path, data) in files {
        push_len_string(&mut output, path);
        output.extend_from_slice(&offset.to_le_bytes());
        output.extend_from_slice(&(data.len() as u32).to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in files {
        output.extend_from_slice(data);
    }
    output
}

fn texture(
    width: u32,
    height: u32,
    content_width: u32,
    content_height: u32,
    pixels: &[u8],
) -> Vec<u8> {
    let mut output = Vec::new();
    push_nul_string(&mut output, "TEXV0005");
    push_nul_string(&mut output, "TEXI0001");
    push_i32(&mut output, 0);
    push_i32(&mut output, 3);
    for value in [width, height, content_width, content_height] {
        push_i32(&mut output, value as i32);
    }
    push_i32(&mut output, 0);
    push_nul_string(&mut output, "TEXB0002");
    push_i32(&mut output, 1);
    push_i32(&mut output, 1);
    push_i32(&mut output, width as i32);
    push_i32(&mut output, height as i32);
    push_i32(&mut output, 0);
    push_i32(&mut output, pixels.len() as i32);
    push_i32(&mut output, pixels.len() as i32);
    output.extend_from_slice(pixels);
    output
}

fn main() -> std::io::Result<()> {
    let output = std::env::args_os()
        .nth(1)
        .map_or_else(|| PathBuf::from("/tmp/skwd-scene-target-fixture"), PathBuf::from);
    std::fs::create_dir_all(&output)?;

    let mut producer = vec![255u8; 64 * 64 * 4];
    for y in 0..64 {
        for x in 0..64 {
            let pixel = &mut producer[(y * 64 + x) * 4..][..4];
            if x < 48 && y < 40 {
                pixel.copy_from_slice(&[(x * 5) as u8, (y * 6) as u8, ((x + y) * 3) as u8, 255]);
            } else {
                pixel.copy_from_slice(&[255, 0, 255, 255]);
            }
        }
    }
    let white = vec![255u8; 4];
    let producer_texture = texture(64, 64, 48, 40, &producer);
    let white_texture = texture(1, 1, 1, 1, &white);
    let scene = br#"{
        "general":{"orthogonalprojection":{"width":48,"height":40},"clearcolor":"0 0 0"},
        "objects":[
            {"id":1,"name":"padded passive","image":"models/producer.json","origin":"24 20 0","size":"48 40","alpha":0},
            {"id":2,"name":"consumer","image":"models/consumer.json","origin":"24 20 1","size":"48 40","effects":[{"file":"effects/target/effect.json"}]}
        ]
    }"#;
    let producer_model = br#"{"material":"materials/producer.json"}"#;
    let consumer_model = br#"{"material":"materials/consumer.json"}"#;
    let producer_material = br#"{"passes":[{"textures":["producer"]}]}"#;
    let consumer_material = br#"{"passes":[{"textures":["white"]}]}"#;
    let effect = br#"{"passes":[{"material":"materials/target.json","bind":[{"name":"_rt_imageLayerComposite_1_a","index":1}]}]}"#;
    let target_material = br#"{"passes":[{"shader":"targetcopy","textures":["white",""]}]}"#;
    let vertex = br"
attribute vec3 a_Position;
attribute vec2 a_TexCoord;
varying vec2 v_TexCoord;
uniform mat4 g_ModelViewProjectionMatrix;
void main() {
    v_TexCoord = a_TexCoord;
    gl_Position = mul(vec4(a_Position, 1.0), g_ModelViewProjectionMatrix);
}
";
    let fragment = br"
varying vec2 v_TexCoord;
uniform sampler2D g_Texture0;
uniform sampler2D g_Texture1;
uniform float g_Time;
void main() {
    gl_FragColor = texSample2D(g_Texture1, v_TexCoord) + vec4(g_Time * 0.0);
}
";
    let bytes = package(&[
        ("scene.json", scene),
        ("models/producer.json", producer_model),
        ("models/consumer.json", consumer_model),
        ("materials/producer.json", producer_material),
        ("materials/consumer.json", consumer_material),
        ("materials/target.json", target_material),
        ("materials/producer.tex", &producer_texture),
        ("materials/white.tex", &white_texture),
        ("effects/target/effect.json", effect),
        ("shaders/targetcopy.vert", vertex),
        ("shaders/targetcopy.frag", fragment),
    ]);
    std::fs::write(output.join("scene.pkg"), bytes)?;
    Ok(())
}
