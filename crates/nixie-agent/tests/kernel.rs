//! Kernel command line parsing.

use nixie_agent::kernel::parse_kernel_params;

#[test]
fn parses_kernel_params() {
    let cases: &[(&str, &[(&str, &str)])] = &[
        (
            "kernel initrd=initrd0 init=/nix/store/nixos-installer-kexec/init loglevel=4 nixie_mac_address=bc:24:11:0d:2f:20 nixie_api=192.168.1.15:5000",
            &[
                ("initrd", "initrd0"),
                ("init", "/nix/store/nixos-installer-kexec/init"),
                ("loglevel", "4"),
                ("nixie_mac_address", "bc:24:11:0d:2f:20"),
                ("nixie_api", "192.168.1.15:5000"),
            ],
        ),
        (
            "kernel nixie_mac_address=aa:bb:cc:dd:ee:ff",
            &[("nixie_mac_address", "aa:bb:cc:dd:ee:ff")],
        ),
        (
            "kernel nixie_api=192.168.1.100:5000",
            &[("nixie_api", "192.168.1.100:5000")],
        ),
        ("kernel root=/dev/sda1 ro quiet", &[("root", "/dev/sda1")]),
        (
            "kernel nixie_api nixie_mac_address=aa:bb:cc:dd:ee:ff",
            &[("nixie_mac_address", "aa:bb:cc:dd:ee:ff")],
        ),
        ("", &[]),
    ];

    for (input, expected) in cases {
        let parsed = parse_kernel_params(input);
        assert_eq!(parsed.len(), expected.len(), "input {input:?}");
        for (key, value) in *expected {
            assert_eq!(
                parsed.get(*key).map(String::as_str),
                Some(*value),
                "input {input:?}"
            );
        }
    }
}
