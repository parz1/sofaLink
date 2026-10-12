"""Read-only Windows focus/XInput evidence; does not inject inputs or steal focus."""
import argparse
import ctypes
from ctypes import wintypes
import json
from pathlib import Path
import time


class Gamepad(ctypes.Structure):
    _fields_=[('buttons',wintypes.WORD),('left_trigger',ctypes.c_ubyte),
              ('right_trigger',ctypes.c_ubyte),('left_x',ctypes.c_short),
              ('left_y',ctypes.c_short),('right_x',ctypes.c_short),('right_y',ctypes.c_short)]


class State(ctypes.Structure):
    _fields_=[('packet',wintypes.DWORD),('pad',Gamepad)]


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--game-pid',type=int,required=True)
    parser.add_argument('--seconds',type=int,default=60)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    if not 1<=args.seconds<=180: parser.error('duration must be 1..180 seconds')
    user=ctypes.WinDLL('user32')
    user.GetForegroundWindow.restype=wintypes.HWND
    user.GetWindowThreadProcessId.argtypes=[wintypes.HWND,ctypes.POINTER(wintypes.DWORD)]
    xinput=ctypes.WinDLL('xinput1_4')
    xinput.XInputGetState.argtypes=[wintypes.DWORD,ctypes.POINTER(State)]
    xinput.XInputGetState.restype=wintypes.DWORD
    args.output.parent.mkdir(parents=True,exist_ok=True)
    deadline=time.monotonic()+args.seconds
    with args.output.open('w',encoding='utf-8') as log:
        while time.monotonic()<deadline:
            foreground=user.GetForegroundWindow()
            owner=wintypes.DWORD()
            user.GetWindowThreadProcessId(foreground,ctypes.byref(owner))
            pads=[]
            for index in range(4):
                state=State()
                if xinput.XInputGetState(index,ctypes.byref(state))==0:
                    pads.append({'index':index,'packet':state.packet,
                                 'buttons':state.pad.buttons,'left_trigger':state.pad.left_trigger,
                                 'right_trigger':state.pad.right_trigger,
                                 'left_x':state.pad.left_x,'left_y':state.pad.left_y,
                                 'right_x':state.pad.right_x,'right_y':state.pad.right_y})
            row={'host_monotonic_ns':time.perf_counter_ns(),'game_foreground':owner.value==args.game_pid,
                 'foreground_window':int(foreground or 0),'pads':pads}
            log.write(json.dumps(row)+'\n');log.flush()
            time.sleep(.1)


if __name__=='__main__': main()
