import setuptools

REQUIRES = [
    'numpy',
    'scenedetect[opencv]',
    'opencv-python',
    'psutil',
    'scipy',
    'python-Levenshtein',
]

with open("README.md", "r") as f:
    long_description = f.read()

setuptools.setup(
    name="libAv1an",
    version="1.13-9",
    author="Master_Of_Zen",
    author_email="master_of_zen@protonmail.com",
    description="All-in-one encode toolkit library",
    long_description=long_description,
    long_description_content_type="text/markdown",
    url="https://github.com/master-of-zen/Av1an",
    packages=setuptools.find_namespace_packages('.', exclude='tests'),
    install_requires=REQUIRES,
    classifiers=[
        "Programming Language :: Python :: 3",
        "License :: OSI Approved :: MIT License",
        "Operating System :: OS Independent",
    ],
    python_requires='>=3.6',
)
