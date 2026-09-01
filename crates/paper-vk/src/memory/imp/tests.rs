use super::clean_nvidia_compiler_mappings;

#[test]
fn clean_compiler_pages() {
    let smaps = "\
7f100000-7f200000 r-xp 00000000 00:01 42 /usr/lib/libnvidia-gpucomp.so.610\n\
Anonymous:             0 kB\n\
Private_Dirty:         0 kB\n\
Shared_Dirty:          0 kB\n";
    let mappings = clean_nvidia_compiler_mappings(smaps);
    let (_, len) = mappings.first().expect("mapping");
    assert_eq!(*len, 0x10_0000);
}

#[test]
fn rejects_dirty_pages() {
    let smaps = "\
7f100000-7f200000 rw-p 0 00:01 42 /usr/lib/libnvidia-gpucomp.so.610\n\
Anonymous:             0 kB\n\
7f200000-7f300000 r--p 0 00:01 42 /usr/lib/libnvidia-gpucomp.so.610\n\
Private_Dirty:         4 kB\n\
7f300000-7f400000 r-xp 0 00:01 42 /usr/lib/libnvidia-gpucomp.so.610\n\
Anonymous:             4 kB\n\
7f400000-7f500000 r-xp 0 00:01 42 /usr/lib/libavcodec.so\n\
Anonymous:             0 kB\n";
    assert!(clean_nvidia_compiler_mappings(smaps).is_empty());
}
