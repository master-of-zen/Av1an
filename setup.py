#!/usr/bin/env python3
import setuptools
import sys

try:
    from setuptools_rust import Binding, RustExtension
except ImportError:
    import subprocess

    errno = subprocess.call([sys.executable, "-m", "pip", "install", "setuptools-rust"])
    if errno:
        print("Please install setuptools-rust package")
        raise SystemExit(errno)
    else:
        from setuptools_rust import Binding, RustExtension


REQUIRES = [
    "numpy",
    "vapoursynth",
    "opencv-python",
    "scipy",
    "maturin",
    "setuptools_rust",
]

setup_requires = ["setuptools-rust", "maturin"]

with open("README.md", "r") as f:
    long_description = f.read()

version = "7.0.0"

setuptools.setup(
    name="Av1an",
    version=version,
    author="Master_Of_Zen",
    author_email="master_of_zen@protonmail.com",
    description="Cross-platform command-line AV1 / VP9 / HEVC / H264 encoding framework with per scene quality encoding",
    long_description=long_description,
    long_description_content_type="text/markdown",
    url="https://github.com/master-of-zen/Av1an",
    packages=setuptools.find_packages(".", exclude="tests"),
    setup_requires=setup_requires,
    install_requires=REQUIRES,
    py_modules=["av1an"],
    rust_extensions=[RustExtension("av1an.av1an", "Cargo.toml", binding=Binding.PyO3)],
    include_package_data=True,
    entry_points={"console_scripts": ["av1an=av1an.py"]},
    classifiers=[
        "Programming Language :: Python :: 3",
        "License :: OSI Approved :: MIT License",
        "Operating System :: OS Independent",
    ],
    python_requires=">=3.6",
    zip_safe=False,
)
