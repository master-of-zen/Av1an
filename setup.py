import setuptools

REQUIRES = [
    'numpy',
    'scenedetect[opencv,progress_bar]',
    'opencv-python',
    'tqdm',
    'psutil',
    'scipy',
    'matplotlib',
]

with open("README.md", "r") as f:
    long_description = f.read()

setuptools.setup(
    name="Av1an",
    version="1.12.13-1",
    author="Master_Of_Zen",
    author_email="master_of_zen@protonmail.com",
    description="All-in-one encode toolkit",
    long_description=long_description,
    long_description_content_type="text/markdown",
    url="https://github.com/master-of-zen/Av1an",
    packages=setuptools.find_packages('.', exclude='tests'),
    install_requires=REQUIRES,
    py_modules=['av1an'],
    entry_points={"console_scripts": ["av1an=av1an:main"]},
    classifiers=[
        "Programming Language :: Python :: 3",
        "License :: OSI Approved :: MIT License",
        "Operating System :: OS Independent",
    ],
    python_requires='>=3.6',
)
