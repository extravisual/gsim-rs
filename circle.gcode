; Creates a circle on the XY plane at machine center

G54;
G90;
X0. Y0.; Move to center

G91;
G1 X100. F50.;

G90; Absolute mode
G3 X-100. Y0. I-100. J0.; Anti clockwise arc with center at G54 offset

G53 X0. Y0.;
M30;
