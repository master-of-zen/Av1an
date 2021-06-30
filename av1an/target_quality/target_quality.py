import fnmatch
import os
import subprocess
from math import isnan

import numpy as np
from av1an.chunk import Chunk
from av1an.manager.Pipes import process_pipe
from av1an.vmaf import VMAF
from av1an_pyo3 import (
    read_weighted_vmaf,
    adapt_probing_rate,
    construct_target_quality_command,
    log,
    probe_cmd,
    vmaf_auto_threads,
    weighted_search,
)
from scipy import interpolate

try:
    import matplotlib
    from matplotlib import pyplot as plt
except ImportError:
    matplotlib = None
    plt = None


class TargetQuality:
    def __init__(self, project):
        self.vmaf_runner = VMAF(
            n_threads=project.n_threads,
            model=project.vmaf_path,
            res=project.vmaf_res,
            vmaf_filter=project.vmaf_filter,
        )
        self.n_threads = project.n_threads
        self.probing_rate = project.probing_rate
        self.probes = project.probes
        self.target = project.target_quality
        self.min_q = project.min_q
        self.max_q = project.max_q
        self.make_plots = project.vmaf_plots
        self.encoder = project.encoder
        self.ffmpeg_pipe = project.ffmpeg_pipe
        self.temp = project.temp
        self.workers = project.workers
        self.video_params = project.video_params

    def log_probes(self, vmaf_cq, frames, name, target_q, target_vmaf, skip=None):
        if skip == "high":
            sk = " Early Skip High CQ"
        elif skip == "low":
            sk = " Early Skip Low CQ"
        else:
            sk = ""

        log(f"Chunk: {name}, Rate: {self.probing_rate}, Fr: {frames}")
        log(f"Probes: {str(sorted(vmaf_cq))[1:-1]}{sk}")
        log(f"Target Q: {target_q} VMAF: {round(target_vmaf, 2)}")

    def per_shot_target_quality(self, chunk: Chunk):
        vmaf_cq = []
        frames = chunk.frames
        self.probing_rate = adapt_probing_rate(self.probing_rate, frames)

        q_list = []

        # Make middle probe
        middle_point = (self.min_q + self.max_q) // 2
        q_list.append(middle_point)
        last_q = middle_point

        score = read_weighted_vmaf(str(self.vmaf_probe(chunk, last_q).as_posix()), 0.25)
        vmaf_cq.append((score, last_q))

        # Initialize search boundary
        vmaf_lower = score
        vmaf_upper = score
        vmaf_cq_lower = last_q
        vmaf_cq_upper = last_q

        # Branch
        if score < self.target:
            next_q = self.min_q
            q_list.append(self.min_q)
        else:
            next_q = self.max_q
            q_list.append(self.max_q)

        # Edge case check
        score = read_weighted_vmaf(str(self.vmaf_probe(chunk, next_q).as_posix()), 0.25)
        vmaf_cq.append((score, next_q))

        if (next_q == self.min_q and score < self.target) or (
            next_q == self.max_q and score > self.target
        ):
            self.log_probes(
                vmaf_cq,
                frames,
                chunk.name,
                next_q,
                score,
                skip="low" if score < self.target else "high",
            )
            return next_q

        # Set boundary
        if score < self.target:
            vmaf_lower = score
            vmaf_cq_lower = next_q
        else:
            vmaf_upper = score
            vmaf_cq_upper = next_q

        # VMAF search
        for _ in range(self.probes - 2):
            new_point = weighted_search(
                vmaf_cq_lower, vmaf_lower, vmaf_cq_upper, vmaf_upper, self.target
            )
            if new_point in [x[1] for x in vmaf_cq]:
                break

            q_list.append(new_point)
            score = read_weighted_vmaf(
                str(self.vmaf_probe(chunk, new_point).as_posix()), 0.25
            )
            vmaf_cq.append((score, new_point))

            # Update boundary
            if score < self.target:
                vmaf_lower = score
                vmaf_cq_lower = new_point
            else:
                vmaf_upper = score
                vmaf_cq_upper = new_point

        q, q_vmaf = self.get_target_q(vmaf_cq, self.target)
        self.log_probes(vmaf_cq, frames, chunk.name, q, q_vmaf)

        return q

    def get_target_q(self, scores, target_quality):
        x = [x[1] for x in sorted(scores)]
        y = [float(x[0]) for x in sorted(scores)]
        f = interpolate.interp1d(x, y, kind="linear")
        xnew = np.linspace(min(x), max(x), max(x) - min(x))
        tl = list(zip(xnew, f(xnew)))
        q = min(tl, key=lambda l: abs(l[1] - target_quality))

        return int(q[0]), round(q[1], 3)

    def vmaf_probe(self, chunk: Chunk, q):

        n_threads = (
            self.n_threads if self.n_threads else vmaf_auto_threads(self.workers)
        )
        cmd = probe_cmd(
            self.encoder,
            str(self.temp.as_posix()),
            chunk.name,
            str(q),
            self.ffmpeg_pipe,
            str(self.probing_rate),
            str(n_threads),
        )
        pipe, utility = self.make_pipes(chunk.ffmpeg_gen_cmd, cmd)
        process_pipe(pipe, chunk, utility)
        probe_name = chunk.temp / "split" / f"v_{q}{chunk.name}.ivf"
        fl = self.vmaf_runner.call_vmaf(chunk, probe_name, vmaf_rate=self.probing_rate)
        return fl

    def interpolate_data(self, vmaf_cq: list, target_quality):
        x = [x[1] for x in sorted(vmaf_cq)]
        y = [float(x[0]) for x in sorted(vmaf_cq)]

        # Interpolate data
        f = interpolate.interp1d(x, y, kind="quadratic")
        xnew = np.linspace(min(x), max(x), max(x) - min(x))

        # Getting value closest to target
        tl = list(zip(xnew, f(xnew)))
        target_quality_cq = min(tl, key=lambda l: abs(l[1] - target_quality))
        return target_quality_cq, tl, f, xnew

    def per_shot_target_quality_routine(self, chunk: Chunk):
        chunk.per_shot_target_quality_cq = self.per_shot_target_quality(chunk)

    def make_pipes(self, ffmpeg_gen_cmd, command):

        ffmpeg_gen_pipe = subprocess.Popen(
            ffmpeg_gen_cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT
        )

        ffmpeg_pipe = subprocess.Popen(
            command[0],
            stdin=ffmpeg_gen_pipe.stdout,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
        )

        pipe = subprocess.Popen(
            command[1],
            stdin=ffmpeg_pipe.stdout,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            universal_newlines=True,
        )

        utility = (ffmpeg_gen_pipe, ffmpeg_pipe)
        return pipe, utility
