# Deep Dive: Virtual Packages and Archspec

When we ran the solver in [Chapter 6](ch06-lock.md), one of the first things it
did was call `VirtualPackage::detect()`. On a typical Linux machine this
produces something like `__linux`, `__glibc =2.38`, and
`__archspec =1 x86_64_v3`. On macOS you'd see `__osx =14.4` and
`__archspec =1 arm`. The solver uses these to filter out packages that are
incompatible with your hardware.

Virtual packages solve a real problem: how do you express that a package
requires a minimum glibc version, or a CUDA GPU, without trying to install
those things?

## The problem

Conda environments are hermetic.  They contain everything needed to run a piece
of software.  But some things can't be installed:

- The Linux kernel
- glibc (you can't replace it at runtime without breaking the whole system)
- The CUDA driver (provided by NVIDIA, not a package)
- macOS SDK features
- CPU instruction set extensions (AVX-512, etc.)

Packages may still require these.  A BLAS library compiled with AVX-512 will
crash on a CPU without it.  A CUDA extension requires both a minimum CUDA
version and a compatible GPU.

## Virtual packages to the rescue

Virtual packages are synthetic packages that represent host capabilities.  They
exist only in the solver's view of the world; they're never installed.
Instead, `rattler_virtual_packages` *detects* them from the system at solve time.

A few examples:

| Virtual package | Represents |
|---|---|
| `__linux` | Linux kernel (presence means "this is Linux") |
| `__glibc =2.38` | GNU C Library version |
| `__osx =14.4` | macOS version |
| `__win` | Windows (presence = "this is Windows") |
| `__cuda =12.3` | CUDA toolkit version |
| `__archspec =1 x86_64_v3` | CPU instruction set level |

A package can declare:
```json
"depends": ["__glibc >=2.17", "__cuda >=11.0"]
```

The solver will reject this package if the host's `__glibc` is older than 2.17
or no `__cuda` virtual package is present.

## How detection works

```rust
let virtual_packages: Vec<GenericVirtualPackage> =
    rattler_virtual_packages::VirtualPackage::detect(
        &rattler_virtual_packages::VirtualPackageOverrides::default(),
    )?
    .into_iter()
    .map(GenericVirtualPackage::from)
    .collect();
```

`VirtualPackage::detect` returns a `Vec<VirtualPackage>`, a typed enum:

```rust
pub enum VirtualPackage {
    Linux(Linux),
    LibC(LibC),
    Osx(Osx),
    Win(Win),
    Cuda(Cuda),
    Archspec(Archspec),
    // ...
}
```

Each variant is detected differently:

- **Linux/Osx/Win**: probe the OS via `std::env::consts::OS`
- **LibC**: parse `/proc/version` or run `ldd --version`
- **Cuda**: read `/proc/driver/nvidia/version` or the CUDA driver API
- **Archspec**: use CPU feature flags from `cpuid` (x86) or `/proc/cpuinfo`

`VirtualPackageOverrides` lets you override the detected values, useful for
testing or cross-compilation scenarios:

```rust
let overrides = VirtualPackageOverrides {
    cuda: Some(Some(Cuda { version: Version::from_str("12.0")? })),
    ..Default::default()
};
```

## `archspec` in Rust

[Archspec] is a microarchitecture specification system.  It maps CPU models to
capability levels:

```text
x86_64          (base 64-bit x86)
x86_64_v2       (SSE4.2, POPCNT, ...)
x86_64_v3       (AVX2, BMI2, ...)
x86_64_v4       (AVX-512, ...)
```

A package compiled with AVX2 goes in the `x86_64_v3` level.  The virtual package
`__archspec =1 x86_64_v3` means "this CPU supports at least the v3 instruction
set".

rattler detects the current CPU level by reading the CPUID instruction (on x86)
or equivalent hardware registers on ARM.  The `archspec` crate wraps this
platform-specific logic.

### Why does `=1` appear?

Archspec uses a two-part version for virtual packages: `__archspec =<gen>
<microarch>`.  The `=1` is the "generation", currently always 1.  The second
part is the microarchitecture name.  This slightly awkward encoding lets archspec
fit into the standard MatchSpec version constraint system.

## `GenericVirtualPackage`

The solver accepts `GenericVirtualPackage`, not the typed enum:

```rust
pub struct GenericVirtualPackage {
    pub name: PackageName,
    pub version: Version,
    pub build_string: String,
}
```

This simpler form is used because the solver doesn't need to know *what kind* of
virtual package it is.  The name and version are sufficient to match against
dependency specs.

## Overriding for cross-compilation

When building packages for a different platform (cross-compiling), you want the
solver to use the *target* platform's virtual packages, not the host's.  The
override mechanism supports this:

```rust
VirtualPackageOverrides {
    // Pretend we're building on linux-64 with glibc 2.17
    libc: Some(Some(LibC {
        family: "glibc".to_string(),
        version: Version::from_str("2.17")?,
    })),
    // Pretend there's no CUDA
    cuda: Some(None),
    ..Default::default()
}
```

`Some(None)` means "override with no package present".  `None` means "use
auto-detection".

## Summary

- Virtual packages represent host capabilities that can't be installed.
- They're detected at solve time by probing the system.
- Packages declare requirements on virtual packages just like regular deps.
- Archspec maps CPUs to capability levels using CPUID/equivalent.
- `GenericVirtualPackage` strips type information for the solver.
[Archspec]: https://github.com/archspec/archspec

- Overrides enable cross-compilation scenarios.
