use super::Source;
use std::collections::BTreeMap;

fn prefix(source: &str) -> Option<u32> {
    Source::parse(source, &BTreeMap::new())?.prefix("bins", 32)
}

#[test]
fn divided_loop_prefix() {
    assert_eq!(
        prefix(
            "#define SIZE 32\n#define COUNT SIZE\nvoid main() { int index; for(index=0;index<COUNT;index+=4) { bins[uint(index)/uint(4)]=vec4(float(index)); } }"
        ),
        Some(8)
    );
}

#[test]
fn floating_alias_prefix() {
    assert_eq!(
        prefix(
            "void main() { float frequency; for(int index=0;index<32;index+=1) { frequency=float(index); amplitude=bins[int(frequency*0.25)][index%int(4)]; value+=sin(frequency); } }"
        ),
        Some(8)
    );
}

#[test]
fn literal_prefix_union() {
    assert_eq!(prefix("void main() { bins[1]=vec4(0); bins[7]=vec4(1); }"), Some(8));
    assert_eq!(prefix("void main() { bins[32]=vec4(0); }"), None);
}

#[test]
fn unknown_accesses_rejected() {
    for body in [
        "bins[index]=vec4(1);",
        "bins[-1]=vec4(1);",
        "consume(bins);",
        "bins[1]=vec4(1); consume(bins);",
        "bins[1]=vec4(1); bins[unknown]=vec4(1);",
    ] {
        assert_eq!(prefix(&format!("void main() {{ {body} }}")), None, "{body}");
    }
}

#[test]
fn unstable_iterators_rejected() {
    for mutation in [
        "index=64;",
        "index++;",
        "++index;",
        "change(index);",
        "change((index));",
        "index>>=1;",
        "index.x=64;",
        "index . x=64;",
        "index[0]=64;",
        "int index=64;",
        "if(flag) index=64;",
        "while(flag) {}",
    ] {
        let source = format!(
            "void main() {{ for(int index=0;index<32;index+=4) {{ {mutation} bins[index/4]=vec4(1); }} }}"
        );
        assert_eq!(prefix(&source), None, "{mutation}");
    }
}

#[test]
fn unstable_macros_rejected() {
    for defines in [
        "#define SIZE 32\n#define SIZE 64",
        "#define SIZE 32\n#undef SIZE",
        "#define SIZE unknown",
        "#define SIZE SIZE",
        "#define SIZE 32\n#define hidden bins",
        "#define SIZE 32\n#define hidden(a,b) a##b",
    ] {
        let source = format!(
            "{defines}\nvoid main() {{ for(int index=0;index<SIZE;index+=4) {{ bins[index/4]=vec4(1); }} }}"
        );
        assert_eq!(prefix(&source), None, "{defines}");
    }
}

#[test]
fn alias_dominance_required() {
    for body in [
        "frequency=float(index); frequency=64; amplitude=bins[int(frequency*0.25)];",
        "if(flag) frequency=float(index); amplitude=bins[int(frequency*0.25)];",
        "amplitude=bins[int(frequency*0.25)];",
        "frequency=float(index); change(frequency); amplitude=bins[int(frequency*0.25)];",
        "frequency=float(index); amplitude=bins[int(frequency*0.3)];",
        "frequency=float(index); amplitude=(frequency=64)+bins[int(frequency*0.25)];",
    ] {
        let source = format!(
            "void main() {{ float frequency; for(int index=0;index<32;index+=1) {{ {body} }} }}"
        );
        assert_eq!(prefix(&source), None, "{body}");
    }
}

#[test]
fn bounded_analysis_work() {
    for head in [
        "int index=0;index<4097;index+=4",
        "int index=0;index<32;index+=0",
        "int index=0;index<=32;index+=4",
        "int index=32;index>0;index-=4",
    ] {
        assert_eq!(
            prefix(&format!("void main() {{ for({head}) {{ bins[index/4]=vec4(1); }} }}")),
            None
        );
    }
    assert_eq!(prefix(&" ".repeat(super::SOURCE_LIMIT + 1)), None);
}

#[test]
fn iterator_scope_required() {
    for declaration in [
        "{ int index; }",
        "{ int index; } float index;",
        "int index; float other,index;",
        "int index;",
    ] {
        let block = if declaration == "int index;" { "{ " } else { "" };
        let closing = if block.is_empty() { "" } else { "}" };
        let source = format!(
            "void main() {{ {declaration} {block}for(index=0;index<32;index+=4) {{ bins[index/4]=vec4(1); }}{closing} }}"
        );
        assert_eq!(prefix(&source), None, "{declaration}");
    }
}

#[test]
fn alias_type_required() {
    for declaration in [
        "half frequency;",
        "float frequency; { half frequency; }",
        "{ float frequency; }",
        "float other=frequency;",
        "#define DECL half frequency\nDECL;",
        "#define float half\nfloat frequency;",
    ] {
        let source = format!(
            "void main() {{\n{declaration}\nfor(int index=0;index<32;index+=1) {{ frequency=float(index); amplitude=bins[int(frequency*0.25)]; }} }}"
        );
        assert_eq!(prefix(&source), None, "{declaration}");
    }
    assert_eq!(
        prefix(
            "#define int half\nvoid main() { for(int index=0;index<3000;index+=3) { bins[index/100]=vec4(1); } }"
        ),
        None
    );
}

#[test]
fn comma_alias_declaration() {
    assert_eq!(
        prefix(
            "void main() { float flow,frequency,wave,total=0; for(int index=0;index<32;index+=1) { frequency=float(index); amplitude=bins[int(frequency*0.25)]; } }"
        ),
        Some(8)
    );
}

#[test]
fn iterator_storage_required() {
    for declarations in [
        "half index; void main() {",
        "float index; void main() {",
        "int index; void main() {",
        "typedef int Counter; void main() { Counter index;",
        "void main() { half index;",
        "void main() { float index;",
        "void main() { int index; { half index;",
    ] {
        let tail = if declarations.ends_with("{ half index;") { "}" } else { "" };
        let source = format!(
            "{declarations} for(index=0;index<32;index+=4) {{ bins[index/4]=vec4(1); }} }} {tail}"
        );
        assert_eq!(prefix(&source), None, "{declarations}");
    }
    let source = "#define mutate(item) item=64\nvoid main() { for(int index=0;index<32;index+=4) { mutate(index); bins[index/4]=vec4(1); } }";
    assert_eq!(prefix(source), None);
}

#[test]
fn loop_macro_rejected() {
    assert_eq!(
        prefix(
            "#define for(header) if(true)\nint index=64;\nvoid main() { for(int index=0;index<32;index+=4) { bins[index/4]=vec4(1); } }"
        ),
        None
    );
}

#[test]
fn array_shadow_rejected() {
    for declaration in ["vec4 bins[1];", "vec4 other[1],bins[1];"] {
        assert_eq!(prefix(&format!("void main() {{ {declaration} bins[0]=vec4(1); }}")), None);
    }
    assert_eq!(prefix("#define JOIN(a,b) a%:%:b\nvoid main() { bins[0]=vec4(1); }"), None);
}

#[test]
fn octal_numbers_rejected() {
    for index in ["index/010", "uint(index)/uint(010)", "010"] {
        let source = format!(
            "void main() {{ for(int index=0;index<64;index+=1) {{ bins[{index}]=vec4(1); }} }}"
        );
        assert_eq!(prefix(&source), None, "{index}");
    }
    assert_eq!(
        prefix(
            "#define DIVISOR 010\nvoid main() { for(int index=0;index<64;index+=1) { bins[index/DIVISOR]=vec4(1); } }"
        ),
        None
    );
}

#[test]
fn commented_defines_rejected() {
    let source = "static const int LIMIT=64;\n/*\n#define LIMIT 32\n*/\nvoid main() { for(int index=0;index<LIMIT;index+=1) { bins[index/4]=vec4(1); } }";
    assert_eq!(prefix(source), None);
    assert_eq!(
        prefix(
            "/* prefix */ #define LIMIT 64\nvoid main() { for(int index=0;index<LIMIT;index+=1) { bins[index/4]=vec4(1); } }"
        ),
        None
    );
}

#[test]
fn late_defines_rejected() {
    for (declaration, bound) in [
        ("static const int LIMIT=64;", "LIMIT"),
        ("#define COUNT LIMIT\nstatic const int LIMIT=64;", "COUNT"),
        ("#define COUNT LIMIT", "COUNT"),
    ] {
        let source = format!(
            "{declaration}\nvoid main() {{ for(int index=0;index<{bound};index+=1) {{ bins[index/4]=vec4(1); }} }}\n#define LIMIT 32"
        );
        assert_eq!(prefix(&source), None, "{declaration}");
    }
}

#[test]
fn line_splices_rejected() {
    let sources = [
        r"static int index; void change() { index=64; }
void main() {
// conceal \
int index;
for(index=0;index<32;index+=4) { change(); bins[index/4]=float4(1); } }",
        r"// ignored \
#define INDEX 0
static const int INDEX=31;
void main() { result=bins[INDEX]; }",
    ];
    for source in sources {
        assert_eq!(prefix(source), None);
        assert_eq!(prefix(&source.replace('\n', "\r\n")), None);
    }
}

#[test]
fn loop_alias_shadow_rejected() {
    let source = "void main() { float frequency; for(int index=0;index<3072;index+=1) { min16float frequency; frequency=float(index); amplitude=bins[int(frequency*0.0078125)]; } }";
    assert_eq!(prefix(source), None);
}
