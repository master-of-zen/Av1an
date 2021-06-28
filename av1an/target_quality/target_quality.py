import subprocess
import os
import fnmatch

from math import isnan
import numpy as np
from scipy import interpolate

from av1an.vmaf import VMAF
from av1an_pyo3 import log
from av1an.chunk import Chunk
from av1an.manager.Pipes import process_pipe
from av1an_pyo3 import (
    adapt_probing_rate,
    construct_target_quality_command,
    construct_target_quality_slow_command,
    vmaf_auto_threads,
    weighted_search,
)

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
        """
        Logs probes result
        :type vmaf_cq: list probe measurements (q_vmaf, q)
        :type frames: int frame count of chunk
        :type name: str chunk name
        :type skip: str None if normal results, else "high" or "low"
        :type target_q: int Calculated q to be used
        :type target_vmaf: float Calculated VMAF that would be achieved by using the q
        :return: None
        """
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
        """
        :type: Chunk chunk to probe
        :rtype: int q to use
        """
        vmaf_cq = []
        frames = chunk.frames
        self.probing_rate = adapt_probing_rate(self.probing_rate, frames)

        q_list = []

        # Make middle probe
        middle_point = (self.min_q + self.max_q) // 2
        q_list.append(middle_point)
        last_q = middle_point

        score = VMAF.read_weighted_vmaf(self.vmaf_probe(chunk, last_q))
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
        score = VMAF.read_weighted_vmaf(self.vmaf_probe(chunk, next_q))
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
            score = VMAF.read_weighted_vmaf(self.vmaf_probe(chunk, new_point))
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
        """
        Interpolating scores to get Q closest to target
        Interpolation type for 2 probes changes to linear
        """
        x = [x[1] for x in sorted(scores)]
        y = [float(x[0]) for x in sorted(scores)]

        if len(x) > 2:
            interpolation = "quadratic"
        else:
            interpolation = "linear"
        f = interpolate.interp1d(x, y, kind=interpolation)
        xnew = np.linspace(min(x), max(x), max(x) - min(x))
        tl = list(zip(xnew, f(xnew)))
        q = min(tl, key=lambda l: abs(l[1] - target_quality))

        return int(q[0]), round(q[1], 3)

    def vmaf_probe(self, chunk: Chunk, q):
        """
        Calculates vmaf and returns path to json file

        :param chunk: the Chunk
        :param q: Value to make probe
        :param project: the Project
        :return : path to json file with vmaf scores
        """

        n_threads = (
            self.n_threads if self.n_threads else vmaf_auto_threads(self.workers)
        )
        cmd = self.probe_cmd(
            chunk, q, self.ffmpeg_pipe, self.encoder, self.probing_rate, n_threads
        )
        pipe, utility = self.make_pipes(chunk.ffmpeg_gen_cmd, cmd)
        process_pipe(pipe, chunk, utility)
        probe_name = chunk.temp / "split" / f"v_{q}{chunk.name}.ivf"
        fl = self.vmaf_runner.call_vmaf(chunk, probe_name, vmaf_rate=self.probing_rate)
        return fl

    def probe_cmd(self, chunk: Chunk, q, ffmpeg_pipe, encoder, probing_rate, n_threads):
        """
        Generate and return commands for probes at set Q values
        These are specifically not the commands that are generated
        by the user or encoder defaults, since these
        should be faster than the actual encoding commands.
        These should not be moved into encoder classes at this point.
        """
        pipe = [
            "ffmpeg",
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            "-",
            "-vf",
            f"select=not(mod(n\\,{probing_rate}))",
            "-vsync",
            "0",
            *ffmpeg_pipe,
        ]

        probe_name = chunk.temp / "split" / f"v_{q}{chunk.name}.ivf"

        if encoder == "aom":
            params = construct_target_quality_command("aom", str(n_threads), str(q))
            cmd = (pipe, [*params, "-o", probe_name, "-"])

        elif encoder == "x265":
            params = construct_target_quality_command("x265", str(n_threads), str(q))
            cmd = (pipe, [*params, "-o", probe_name, "-"])

        elif encoder == "rav1e":
            params = construct_target_quality_command("rav1e", str(n_threads), str(q))
            cmd = (pipe, [*params, "-o", probe_name, "-"])

        elif encoder == "vpx":
            params = construct_target_quality_command("vpx", str(n_threads), str(q))
            cmd = (pipe, [*params, "-o", probe_name, "-"])

        elif encoder == "svt_av1":
            params = construct_target_quality_command("svt_av1", str(n_threads), str(q))
            cmd = (pipe, [*params, "-b", probe_name])

        elif encoder == "x264":
            params = construct_target_quality_command("x264", str(n_threads), str(q))
            cmd = (pipe, [*params, "-o", probe_name, "-"])

        return cmd

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
        """
        Applies per_shot_target_quality to this chunk. Determines what the cq value should be and sets the
        per_shot_target_quality_cq for this chunk

        :param project: the Project
        :param chunk: the Chunk
        :return: None
        """
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
