# brachiograph

A library for brachiographs.

## The geometry

Our brachiograph is parameterized like this:

![A drawing of brachiograph geometry](./drawing.svg)
<img src="./drawing.svg">

The filled black circle is at the origin (and the shoulder joint), and the white circle is at the hand.
We'll call the bit between the shoulder and the elbow the "humerus" and denote its length by $\ell_h$,
and the part between the elbow and the hand is the "ulna", and its length is $\ell_u$.
The counter-clockwise angle between the humerus and the negative $x$ axis is called $\phi_s$, while the angle between
the counter-clockwise $y$ axis and the ulna is called $-\phi_e$.
The point of these conventions is that the "resting" position (in which the humerus points to the left and the ulna points up)
has $\phi_s = \phi_e = 0$; and the reason behind the sign of $\phi_e$ is because the rotating tip of the elbow servo is
glued to the humerus. That is, if we tell the elbow servo to rotate $\phi$ degrees counter-clockwise then the ulna (which
is attached to the body of the servo and not the turny part) will turn clockwise instead. With our conventions,
$\phi_s$ and $\phi_e$ (without the minus sign) refer to the servos' rotation angles.

TODO: derive the formulas
