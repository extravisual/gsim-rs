; Creates a circle on the XY plane at machine center

G54;
G90;
X0. Y0.; Move to center

G91;
G19; YZ Plane

G1 Y100. F50.;

G90; Absolute mode
G2 X0. Y-100. J-100. K0.; Anti clockwise arc with center at G54 offset

;G2 X0. Y100. I100. J0.; Anti clockwise arc with center at G54 offset

;G2 X100. Y0. I0. J-100.; Anti clockwise arc with center at G54 offset

G53 X0. Y0.;
M30;
