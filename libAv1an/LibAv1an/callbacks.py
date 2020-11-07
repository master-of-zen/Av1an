
class Callbacks(object):
    def __init__(self):
        # log - Called when logging is requested - str: message to log
        #
        # newtask - Called whenever task changes
        # str: Current av1an task name - int: total datums to complete task (eg frames, percent, etc)
        #
        # newframes - Called when new frames are rendered - int: Number of completed frames since last call.
        # eg if there were 4 frames done before and now 7 frames, this will pass 3.
        #
        # terminate - Called when av1an fails on a task or completes its job - int: 0 if success, otherwise error code.
        #
        # plotvmaf - Called when vmaf is calculated so frontend can do plots - int: target vmaf - int: min_q
        # int: max_q - path: tempdir - List[Tuple[Float, Int]]: plot data - str: name of chunk - int: number of frames
        #
        # logready - Called when tmp is created so logging can be done there - Path: log path set in args - Path: tmp
        #
        # startencode - Called when encode starts - int: total frames - int: start frames
        #
        # endencode - Called when encode ends - void
        #
        # plotvmaffile - Called when vmaf file plotted - Path: input file - Path: expected output file
        self.subscriptions = {'log': {}, 'newtask': {}, 'newframes': {}, 'terminate': {}, 'plotvmaf': {},
                              'logready': {}, 'startencode': {}, 'endencode': {}, 'svtvp9update': {},
                              'plotvmaffile': {}}

    def subscribe(self, hook, function: classmethod, funcname=""):
        if len(funcname) > 0:
            self.subscriptions[hook][funcname] = function
        else:
            self.subscriptions[hook][function.__name__] = function

    def unsubscribe(self, hook, function: classmethod, funcname=""):
        if len(funcname) > 0:
            del(self.subscriptions[hook][funcname])
        else:
            del(self.subscriptions[hook][function.__name__])

    def run_callback(self, hook, *args):
        for key in self.subscriptions[hook]:
            self.subscriptions[hook][key](*args)

