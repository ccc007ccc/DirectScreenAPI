# key|default|type|label|description
TOUCH_DEMO_FILTER_CHAIN||text|Filter Chain|默认关闭链式模糊；留空时走 FILTER_CLEAR
TOUCH_DEMO_BLUR_RADIUS|0|number|Blur Radius|presenter 全局模糊半径，默认 0（关闭）
TOUCH_DEMO_BLUR_SIGMA|0|number|Blur Sigma|仅配合 chain 使用；默认 0
TOUCH_DEMO_PRESENTER_FRAME_RATE|current|text|Presenter Frame Rate|默认 current；可选 auto/max/current/具体帧率
TOUCH_DEMO_FPS|60|number|Demo FPS|默认 60；0=按屏幕刷新率运行
TOUCH_DEMO_NO_TOUCH_ROUTER|0|bool|No Touch Router|1=仅渲染，不接管触控
TOUCH_DEMO_DEVICE||text|Input Device|指定触摸设备路径，如 /dev/input/event7
TOUCH_DEMO_WINDOW_X||number|Window X|初始窗口 X
TOUCH_DEMO_WINDOW_Y||number|Window Y|初始窗口 Y
TOUCH_DEMO_WINDOW_W|620|number|Window Width|初始窗口宽度
TOUCH_DEMO_WINDOW_H|440|number|Window Height|初始窗口高度
TOUCH_DEMO_ROUTE_NAME|demo-window|text|Route Name|触摸路由名称
TOUCH_DEMO_LAYER_NAME|DirectScreenAPI|text|Layer Name|presenter layer name
TOUCH_DEMO_LAYER_Z|1000000|number|Layer Z|presenter layer z
TOUCH_DEMO_RUN_SECONDS|0|number|Run Seconds|0=不自动退出
TOUCH_DEMO_STOP_DAEMON|0|bool|Stop Daemon On Stop|stop action 时是否顺带停止核心
