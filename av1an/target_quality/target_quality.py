import subprocess
import os

from math import isnan
import numpy as np
from scipy import interpolate

from av1an.vmaf import VMAF
from av1an.logger import log
from av1an.commandtypes import CommandPair, Command
from av1an.chunk import Chunk
from av1an.manager.Pipes import process_pipe
from av1an_pyo3 import adapt_probing_rate, construct_target_quality_command

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
            new_point = self.weighted_search(
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

        # Plot Probes
        if self.make_plots and len(vmaf_cq) > 3:
            self.plot_probes(vmaf_cq, chunk, frames)

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

    def weighted_search(self, num1, vmaf1, num2, vmaf2, target):
        """
        Returns weighted value closest to searched

        :param num1: Q of first probe
        :param vmaf1: VMAF of first probe
        :param num2: Q of second probe
        :param vmaf2: VMAF of first probe
        :param target: VMAF target
        :return: Q for new probe
        """

        dif1 = abs(VMAF.transform_vmaf(target) - VMAF.transform_vmaf(vmaf2))
        dif2 = abs(VMAF.transform_vmaf(target) - VMAF.transform_vmaf(vmaf1))

        tot = dif1 + dif2

        new_point = int(round(num1 * (dif1 / tot) + (num2 * (dif2 / tot))))
        return new_point

    def vmaf_probe(self, chunk: Chunk, q):
        """
        Calculates vmaf and returns path to json file

        :param chunk: the Chunk
        :param q: Value to make probe
        :param project: the Project
        :return : path to json file with vmaf scores
        """

        n_threads = self.n_threads if self.n_threads else self.auto_vmaf_threads()
        cmd = self.probe_cmd(
            chunk, q, self.ffmpeg_pipe, self.encoder, self.probing_rate, n_threads
        )
        pipe, utility = self.make_pipes(chunk.ffmpeg_gen_cmd, cmd)
        process_pipe(pipe, chunk, utility)
        fl = self.vmaf_runner.call_vmaf(
            chunk, self.gen_probes_names(chunk, q), vmaf_rate=self.probing_rate
        )
        return fl

    def auto_vmaf_threads(self):
        """
        Calculates number of vmaf threads based on CPU cores in system

        :return: Integer value for number of threads
        """
        cores = os.cpu_count()
        # One thread may not be enough to keep the CPU saturated, so over-provision a bit.
        over_provision_factor = 1.25
        minimum_threads = 1

        return int(max((cores / self.workers) * over_provision_factor, minimum_threads))

    def get_closest(self, q_list, q, positive=True):
        """
        Returns closest value from the list, ascending or descending

        :param q_list: list of q values that been already used
        :param q:
        :param positive: search direction, positive - only values bigger than q
        :return: q value from list
        """
        if positive:
            q_list = [x for x in q_list if x > q]
        else:
            q_list = [x for x in q_list if x < q]

        return min(q_list, key=lambda x: abs(x - q))

    def probe_cmd(
        self, chunk: Chunk, q, ffmpeg_pipe, encoder, probing_rate, n_threads
    ) -> CommandPair:
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
            *ffmpeg_pipe,
        ]

        probe_name = self.gen_probes_names(chunk, q).with_suffix(".ivf").as_posix()

        if encoder == "aom":
            params = construct_target_quality_command("aom", str(n_threads), str(q))
            cmd = CommandPair(pipe, [*params, "-o", probe_name, "-"])

        elif encoder == "x265":
            params = construct_target_quality_command("x265", str(n_threads), str(q))
            cmd = CommandPair(pipe, [*params, "-o", probe_name, "-"])

        elif encoder == "rav1e":
            params = construct_target_quality_command("rav1e", str(n_threads), str(q))
            cmd = CommandPair(pipe, [*params, "-o", probe_name, "-"])

        elif encoder == "vpx":
            params = construct_target_quality_command("vpx", str(n_threads), str(q))
            cmd = CommandPair(pipe, [*params, "-o", probe_name, "-"])

        elif encoder == "svt_av1":
            params = construct_target_quality_command("svt_av1", str(n_threads), str(q))
            cmd = CommandPair(pipe, [*params, "-b", probe_name])

        elif encoder == "x264":
            params = construct_target_quality_command("x264", str(n_threads), str(q))
            cmd = CommandPair(pipe, [*params, "-o", probe_name, "-"])

        return cmd

    def search(self, q1, v1, q2, v2, target):

        if abs(target - v2) < 0.5:
            return q2

        if v1 > target and v2 > target:
            return min(q1, q2)
        if v1 < target and v2 < target:
            return max(q1, q2)

        dif1 = abs(target - v2)
        dif2 = abs(target - v1)

        tot = dif1 + dif2

        new_point = int(round(q1 * (dif1 / tot) + (q2 * (dif2 / tot))))
        return new_point

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

    def gen_probes_names(self, chunk: Chunk, q):
        """
        Make name of vmaf probe
        """
        return chunk.fake_input_path.with_name(f"v_{q}{chunk.name}").with_suffix(".ivf")

    def make_pipes(self, ffmpeg_gen_cmd: Command, command: CommandPair):

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

    def plot_probes(self, vmaf_cq, chunk: Chunk, frames):
        """
        Makes graph with probe decisions
        """
        if plt is None:
            log(
                "Matplotlib is not installed or could not be loaded\
            . Unable to plot probes."
            )
            return
        # Saving plot of vmaf calculation

        x = [x[1] for x in sorted(vmaf_cq)]
        y = [float(x[0]) for x in sorted(vmaf_cq)]

        cq, tl, f, xnew = self.interpolate_data(vmaf_cq, self.target)
        matplotlib.use("agg")
        plt.ioff()
        plt.plot(xnew, f(xnew), color="tab:blue", alpha=1)
        plt.plot(x, y, "p", color="tab:green", alpha=1)
        plt.plot(cq[0], cq[1], "o", color="red", alpha=1)
        plt.grid(True)
        plt.xlim(self.min_q, self.max_q)
        vmafs = [int(x[1]) for x in tl if isinstance(x[1], float) and not isnan(x[1])]
        plt.ylim(min(vmafs), max(vmafs) + 1)
        plt.ylabel("VMAF")
        plt.title(f"Chunk: {chunk.name}, Frames: {frames}")
        plt.xticks(np.arange(self.min_q, self.max_q + 1, 1.0))
        temp = self.temp / chunk.name
        plt.savefig(f"{temp}.png", dpi=200, format="png")
        plt.close()

    def add_probes_to_frame_list(self, frame_list, q_list, vmafs):
        frame_list = list(frame_list)
        for index, q_vmaf in enumerate(zip(q_list, vmafs)):
            frame_list[index]["probes"].append((q_vmaf[0], q_vmaf[1]))

        return frame_list

    def get_square_error(self, ls, target):
        total = 0
        for i in ls:
            dif = i - target
            total += dif ** 2
        mse = total / len(ls)
        return mse

    def gen_next_q(self, frame_list, chunk):
        q_list = []

        probes = len(frame_list[0]["probes"])

        if probes == 0:
            return [self.min_q] * len(frame_list)
        elif probes == 1:
            return [self.max_q] * len(frame_list)
        else:
            for probe in frame_list:

                x = [x[0] for x in probe["probes"]]
                y = [x[1] for x in probe["probes"]]

                if probes > 2:
                    if len(x) != len(set(x)):
                        q_list.append(probe["probes"][-1][0])
                        continue

                interpolation = "quadratic" if probes > 2 else "linear"

                f = interpolate.interp1d(x, y, kind=interpolation)
                xnew = np.linspace(min(x), max(x), max(x) - min(x))
                tl = list(zip(xnew, f(xnew)))
                q = min(tl, key=lambda l: abs(l[1] - self.target))

                q_list.append(int(round(q[0])))

            return q_list
